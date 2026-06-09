//! SEC EDGAR MCP Server
//!
//! Exposes SEC EDGAR API data as MCP tools over stdio using the MCP JSON-RPC
//! protocol.
//!
//! On first use of any data tool, the client will call `sec_configure` to ask
//! the user for their contact email. This is required by the SEC EDGAR
//! fair-access policy (https://www.sec.gov/os/accessing-edgar-data). The email
//! is stored in the platform config directory (e.g.
//! `~/Library/Application Support/sec-mcp/config.toml` on macOS,
//! `~/.config/sec-mcp/config.toml` on Linux) and used as the HTTP User-Agent
//! contact.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The latest MCP protocol revision this server implements. Used as the
/// fallback when the client requests a version we don't recognize.
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
/// Protocol revisions this server is compatible with, newest first. During
/// `initialize` we echo back the client's requested version if it appears
/// here; otherwise we offer our latest (`MCP_PROTOCOL_VERSION`) and let the
/// client decide whether to proceed.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const TICKER_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    contact_email: Option<String>,
}

impl Config {
    fn path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("cannot determine config directory")?;
        Ok(base.join("sec-mcp").join("config.toml"))
    }

    fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    fn try_load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }

    fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }
}

// ── SEC EDGAR API client ──────────────────────────────────────────────────────

struct EdgarClient {
    http: reqwest::Client,
    ticker_cache: RwLock<Option<(Instant, HashMap<String, String>)>>,
}

impl EdgarClient {
    fn new(contact_email: &str) -> Result<Self> {
        let user_agent = format!("sec-mcp/{SERVER_VERSION} (contact: {contact_email})");
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(HTTP_TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            ticker_cache: RwLock::new(None),
        })
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("HTTP request failed: {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            match status.as_u16() {
                404 => anyhow::bail!("SEC EDGAR returned 404 (not found) for {url}"),
                429 => anyhow::bail!(
                    "SEC EDGAR rate limit exceeded (429). The fair-access limit is 10 req/sec; retry shortly."
                ),
                _ => anyhow::bail!("SEC EDGAR returned {status} for {url}: {snippet}"),
            }
        }
        resp.json()
            .await
            .with_context(|| format!("failed to parse JSON response from {url}"))
    }

    async fn ticker_map(&self) -> Result<HashMap<String, String>> {
        {
            let guard = self.ticker_cache.read().await;
            if let Some((fetched, map)) = guard.as_ref() {
                if fetched.elapsed() < TICKER_CACHE_TTL {
                    return Ok(map.clone());
                }
            }
        }

        let url = "https://www.sec.gov/files/company_tickers.json";
        let raw: HashMap<String, Value> = self
            .get_json(url)
            .await?
            .as_object()
            .context("ticker file was not a JSON object")?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut map = HashMap::with_capacity(raw.len());
        for entry in raw.values() {
            let ticker = entry.get("ticker").and_then(|v| v.as_str());
            let cik = entry.get("cik_str").and_then(|v| v.as_u64());
            if let (Some(t), Some(c)) = (ticker, cik) {
                map.insert(t.to_uppercase(), format!("{c:010}"));
            }
        }

        let mut guard = self.ticker_cache.write().await;
        *guard = Some((Instant::now(), map.clone()));
        Ok(map)
    }

    async fn cik_for_ticker(&self, ticker: &str) -> Result<String> {
        let upper = ticker.to_uppercase();
        let map = self.ticker_map().await?;
        map.get(&upper)
            .cloned()
            .with_context(|| format!("ticker '{ticker}' not found in EDGAR"))
    }

    async fn recent_filings(&self, cik: &str) -> Result<Value> {
        let url = format!("https://data.sec.gov/submissions/CIK{cik}.json");
        self.get_json(&url).await
    }

    /// Fetch one of the older-history submission files named in
    /// `filings.files[]` (e.g. `CIK0000320193-submissions-001.json`).
    async fn submissions_file(&self, name: &str) -> Result<Value> {
        let url = format!("https://data.sec.gov/submissions/{name}");
        self.get_json(&url).await
    }

    async fn company_concept(&self, cik: &str, taxonomy: &str, concept: &str) -> Result<Value> {
        // `taxonomy` and `concept` are caller-supplied (ultimately model-
        // controlled), so percent-encode them before they enter the path.
        // `cik` is a resolved 10-digit number and needs no encoding.
        let taxonomy = urlencoding::encode(taxonomy);
        let concept = urlencoding::encode(concept);
        let url = format!(
            "https://data.sec.gov/api/xbrl/companyconcept/CIK{cik}/{taxonomy}/{concept}.json"
        );
        self.get_json(&url).await
    }

    async fn list_tickers(&self) -> Result<Value> {
        let url = "https://www.sec.gov/files/company_tickers_exchange.json";
        self.get_json(url).await
    }

    async fn company_facts(&self, cik: &str) -> Result<Value> {
        let url = format!("https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json");
        self.get_json(&url).await
    }

    /// `period_code` must be the full SEC frames period code without the leading
    /// `CY` (e.g. `2024`, `2024Q1`, `2024Q1I`). Instant concepts (Assets,
    /// StockholdersEquity, etc.) require the trailing `I`; duration concepts
    /// (Revenues, NetIncomeLoss, etc.) must not have it.
    async fn xbrl_frames(
        &self,
        taxonomy: &str,
        concept: &str,
        unit: &str,
        period_code: &str,
    ) -> Result<Value> {
        // All four segments are caller-supplied; percent-encode each so a
        // crafted value can't inject path separators or other URL syntax. The
        // literal `CY` prefix stays outside the encoded segment.
        let taxonomy = urlencoding::encode(taxonomy);
        let concept = urlencoding::encode(concept);
        let unit = urlencoding::encode(unit);
        let period_code = urlencoding::encode(period_code);
        let url = format!(
            "https://data.sec.gov/api/xbrl/frames/{taxonomy}/{concept}/{unit}/CY{period_code}.json"
        );
        self.get_json(&url).await
    }
}

// ── Shared server state ───────────────────────────────────────────────────────

struct State {
    config: Config,
    client: Option<Arc<EdgarClient>>,
}

impl State {
    fn new() -> Self {
        let config = Config::load();
        let client = config
            .contact_email
            .as_deref()
            .and_then(|email| EdgarClient::new(email).ok())
            .map(Arc::new);
        Self { config, client }
    }

    fn is_configured(&self) -> bool {
        self.config.contact_email.is_some()
    }

    fn set_email(&mut self, email: String) -> Result<()> {
        self.client = Some(Arc::new(EdgarClient::new(&email)?));
        self.config.contact_email = Some(email);
        self.config.save()?;
        Ok(())
    }

    fn client(&self) -> Result<Arc<EdgarClient>> {
        self.client.clone().context(
            "SEC EDGAR contact email not configured. \
             Please call the sec_configure tool first.",
        )
    }
}

// ── Tool definitions ──────────────────────────────────────────────────────────

fn tool_list(configured: bool) -> Value {
    let configure_desc = if configured {
        "Update the contact email used in SEC EDGAR HTTP requests. \
         The current email is already set — only call this if you need to change it."
    } else {
        "REQUIRED SETUP: Register a contact email for SEC EDGAR API access. \
         The SEC fair-access policy requires all automated clients to identify \
         themselves with a contact address. Call this tool before using any \
         other SEC tools. Ask the user for permission and their email address first."
    };

    json!({
        "tools": [
            {
                "name": "sec_configure",
                "description": configure_desc,
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "contact_email": {
                            "type": "string",
                            "description": "Email address to include in SEC EDGAR HTTP User-Agent header"
                        }
                    },
                    "required": ["contact_email"]
                }
            },
            {
                "name": "sec_lookup_cik",
                "description": "Look up the SEC CIK (Central Index Key) number for a company by its stock ticker symbol.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ticker": { "type": "string", "description": "Stock ticker symbol, e.g. AAPL, MSFT, TSLA" }
                    },
                    "required": ["ticker"]
                }
            },
            {
                "name": "sec_recent_filings",
                "description": "Get recent SEC filings (10-K, 10-Q, 8-K, etc.) for a company by ticker symbol. Returns the most recent filings first. When a form_type filter is given, older history is paged in automatically if recent filings don't satisfy the requested limit.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ticker": { "type": "string", "description": "Stock ticker symbol" },
                        "form_type": { "type": "string", "description": "Optional filter: '10-K', '10-Q', '8-K', etc." },
                        "limit": { "type": "integer", "description": "Number of filings to return (default 10, max 40)", "default": 10 }
                    },
                    "required": ["ticker"]
                }
            },
            {
                "name": "sec_financial_concept",
                "description": "Get historical values for a financial concept from SEC XBRL data. Common concepts: 'Revenues', 'NetIncomeLoss', 'EarningsPerShareBasic', 'Assets', 'StockholdersEquity', 'OperatingIncomeLoss', 'CashAndCashEquivalentsAtCarryingValue'.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ticker": { "type": "string", "description": "Stock ticker symbol" },
                        "concept": { "type": "string", "description": "XBRL concept name, e.g. 'Revenues', 'NetIncomeLoss'" },
                        "taxonomy": { "type": "string", "description": "Taxonomy: 'us-gaap' (default) or 'ifrs-full'", "default": "us-gaap" },
                        "period": { "type": "string", "description": "Filter by period: 'annual' (10-K) or 'quarterly' (10-Q)", "enum": ["annual", "quarterly"] }
                    },
                    "required": ["ticker", "concept"]
                }
            },
            {
                "name": "sec_company_info",
                "description": "Get general information about a public company from SEC EDGAR: SIC code, industry, state of incorporation, fiscal year end, addresses, and exchange listings.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ticker": { "type": "string", "description": "Stock ticker symbol" }
                    },
                    "required": ["ticker"]
                }
            },
            {
                "name": "sec_list_tickers",
                "description": "List all active SEC-registered tickers with exchange info, optionally filtered by search query. Useful for finding a company's ticker symbol or seeing what's listed on a particular exchange.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Optional filter: match against ticker or company name (case-insensitive substring)" }
                    }
                }
            },
            {
                "name": "sec_company_facts",
                "description": "Get all available XBRL financial facts for a company — useful for discovering what concepts (metrics) a company reports before querying specific values with sec_financial_concept.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ticker": { "type": "string", "description": "Stock ticker symbol" },
                        "taxonomy": { "type": "string", "description": "Taxonomy to list: 'us-gaap' (default), 'ifrs-full', 'dei', or omit to list all", "default": "us-gaap" }
                    },
                    "required": ["ticker"]
                }
            },
            {
                "name": "sec_xbrl_frames",
                "description": "Cross-company comparison — get a specific financial metric for all companies in a given period. For example, compare revenue across all filers for 2024. Returns top entries sorted by value. Note: 'instant' concepts (balance-sheet items measured at a point in time, e.g. Assets, Liabilities, StockholdersEquity, CashAndCashEquivalentsAtCarryingValue, CommonStockSharesOutstanding) require instant=true with a quarterly period. 'Duration' concepts (flow items measured over a period, e.g. Revenues, NetIncomeLoss, OperatingIncomeLoss, EarningsPerShareBasic) require instant=false (the default).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "concept": { "type": "string", "description": "XBRL concept name, e.g. 'Revenues', 'NetIncomeLoss', 'Assets'" },
                        "period": { "type": "string", "description": "Period: year like '2024' (annual, duration concepts only), or quarter like '2024Q1'" },
                        "instant": { "type": "boolean", "description": "Set true for instant/balance-sheet concepts measured at a point in time (must be combined with a quarterly period). Default false.", "default": false },
                        "taxonomy": { "type": "string", "description": "Taxonomy: 'us-gaap' (default) or 'ifrs-full'", "default": "us-gaap" },
                        "unit": { "type": "string", "description": "Unit: 'USD' (default), 'pure', 'shares', etc.", "default": "USD" },
                        "limit": { "type": "integer", "description": "Number of top entries to return (default 20)", "default": 20 }
                    },
                    "required": ["concept", "period"]
                }
            }
        ]
    })
}

// ── Tool handlers ─────────────────────────────────────────────────────────────

/// Append row-shaped filings from one column-major submissions block into
/// `out`, honoring `form_filter`, until `out` reaches `limit`. The block is
/// either `filings.recent` from the main submissions file or the top-level
/// object of an older-history file — both share the same parallel-array layout.
fn collect_filings(
    block: &Value,
    cik_num: &str,
    form_filter: Option<&str>,
    limit: usize,
    out: &mut Vec<Value>,
) {
    let forms = match block["form"].as_array() {
        Some(f) => f,
        None => return,
    };
    let col = |key: &str| block[key].as_array();
    let dates = col("filingDate");
    let accs = col("accessionNumber");
    let docs = col("primaryDocument");
    let descs = col("primaryDocDescription");
    let cell = |arr: Option<&Vec<Value>>, i: usize| {
        arr.and_then(|a| a.get(i)).cloned().unwrap_or(Value::Null)
    };

    for (i, f) in forms.iter().enumerate() {
        if out.len() >= limit {
            return;
        }
        if let Some(ft) = form_filter {
            if f.as_str().unwrap_or("") != ft {
                continue;
            }
        }
        let acc_raw = accs
            .and_then(|a| a.get(i))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let acc_nodash = acc_raw.replace('-', "");
        let doc = docs
            .and_then(|a| a.get(i))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url =
            format!("https://www.sec.gov/Archives/edgar/data/{cik_num}/{acc_nodash}/{doc}");
        out.push(json!({
            "form": f,
            "date": cell(dates, i),
            "description": cell(descs, i),
            "accession_number": acc_raw,
            "url": url
        }));
    }
}

async fn handle_tool(state: &Arc<RwLock<State>>, name: &str, args: &Value) -> Result<Value> {
    match name {
        "sec_configure" => {
            let email = args["contact_email"]
                .as_str()
                .context("contact_email required")?
                .trim()
                .to_string();

            if email.is_empty() {
                anyhow::bail!("contact_email cannot be empty");
            }

            let mut s = state.write().await;
            s.set_email(email.clone())
                .context("failed to save configuration")?;

            info!("contact email configured: {email}");

            Ok(json!({
                "status": "configured",
                "contact_email": email,
                "config_path": Config::path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "unknown".into()),
                "message": format!(
                    "SEC EDGAR contact email saved. \
                     All requests will now identify as: sec-mcp/{SERVER_VERSION} (contact: {email})"
                )
            }))
        }

        "sec_lookup_cik" => {
            let client = state.read().await.client()?;
            let ticker = args["ticker"].as_str().context("ticker required")?;
            let cik = client.cik_for_ticker(ticker).await?;
            Ok(json!({
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "edgar_url": format!(
                    "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}"
                )
            }))
        }

        "sec_recent_filings" => {
            let client = state.read().await.client()?;
            let ticker = args["ticker"]
                .as_str()
                .context("ticker required")?
                .to_string();
            let form_filter_owned = args["form_type"].as_str().map(|s| s.to_string());
            let limit = args["limit"].as_u64().unwrap_or(10).min(40) as usize;
            let cik = client.cik_for_ticker(&ticker).await?;
            let data = client.recent_filings(&cik).await?;

            let form_filter = form_filter_owned.as_deref();
            let company_name = data["name"].as_str().unwrap_or("Unknown").to_string();
            let cik_num = cik.trim_start_matches('0').to_string();

            // The main submissions file holds only the most recent ~1000
            // filings in `filings.recent`. Start there; it satisfies the
            // common case in a single request.
            let mut filings: Vec<Value> = Vec::with_capacity(limit);
            collect_filings(
                &data["filings"]["recent"],
                &cik_num,
                form_filter,
                limit,
                &mut filings,
            );

            // If the recent window didn't fill the request — typically a
            // `form_type` filter whose matches predate it — page back through
            // the older-history files (listed newest-first) until we hit the
            // limit or run out. Only fetched when actually needed.
            let mut pages_fetched = 0usize;
            if filings.len() < limit {
                if let Some(files) = data["filings"]["files"].as_array() {
                    for file in files {
                        if filings.len() >= limit {
                            break;
                        }
                        let Some(name) = file["name"].as_str() else {
                            continue;
                        };
                        let older = client.submissions_file(name).await?;
                        pages_fetched += 1;
                        collect_filings(&older, &cik_num, form_filter, limit, &mut filings);
                    }
                }
            }

            if pages_fetched > 0 {
                debug!("paged {pages_fetched} older-history file(s) for {ticker}");
            }

            Ok(json!({
                "company": company_name,
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "filings": filings
            }))
        }

        "sec_financial_concept" => {
            let client = state.read().await.client()?;
            let ticker = args["ticker"]
                .as_str()
                .context("ticker required")?
                .to_string();
            let concept = args["concept"]
                .as_str()
                .context("concept required")?
                .to_string();
            let taxonomy = args["taxonomy"].as_str().unwrap_or("us-gaap").to_string();
            let period_filter_owned = args["period"].as_str().map(|s| s.to_string());
            let cik = client.cik_for_ticker(&ticker).await?;
            let data = client.company_concept(&cik, &taxonomy, &concept).await?;

            let period_filter = period_filter_owned.as_deref();
            let entity = data["entityName"].as_str().unwrap_or("Unknown").to_string();
            let label = data["label"].as_str().unwrap_or(&concept).to_string();
            let description = data["description"].as_str().unwrap_or("").to_string();

            let units_obj = data["units"].as_object();
            let (unit_name, entries) = units_obj
                .and_then(|u| {
                    u.get("USD")
                        .map(|v| ("USD", v))
                        .or_else(|| u.get("pure").map(|v| ("pure", v)))
                        .or_else(|| u.iter().next().map(|(k, v)| (k.as_str(), v)))
                })
                .and_then(|(k, v)| v.as_array().map(|a| (k, a.clone())))
                .unwrap_or(("unknown", vec![]));

            let filtered: Vec<&Value> = entries
                .iter()
                .filter(|e| match period_filter {
                    Some("annual") => e["form"].as_str() == Some("10-K"),
                    Some("quarterly") => e["form"].as_str() == Some("10-Q"),
                    _ => true,
                })
                .collect();

            let tail: Vec<&Value> = filtered.iter().rev().take(20).rev().copied().collect();

            Ok(json!({
                "company": entity,
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "concept": concept,
                "taxonomy": taxonomy,
                "label": label,
                "description": description,
                "unit": unit_name,
                "data": tail
            }))
        }

        "sec_company_info" => {
            let client = state.read().await.client()?;
            let ticker = args["ticker"]
                .as_str()
                .context("ticker required")?
                .to_string();
            let cik = client.cik_for_ticker(&ticker).await?;
            let data = client.recent_filings(&cik).await?;

            Ok(json!({
                "name": data["name"],
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "sic": data["sic"],
                "sic_description": data["sicDescription"],
                "state_of_incorporation": data["stateOfIncorporation"],
                "fiscal_year_end": data["fiscalYearEnd"],
                "business_address": data["addresses"]["business"],
                "mailing_address": data["addresses"]["mailing"],
                "exchanges": data["exchanges"],
                "tickers": data["tickers"],
                "edgar_page": format!(
                    "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}"
                )
            }))
        }

        "sec_list_tickers" => {
            let client = state.read().await.client()?;
            let data = client.list_tickers().await?;

            let query = args["query"].as_str().map(|s| s.to_lowercase());
            let rows = data["data"].as_array();

            let results: Vec<Value> = match rows {
                Some(rows) => rows
                    .iter()
                    .filter(|row| {
                        let row = match row.as_array() {
                            Some(r) => r,
                            None => return false,
                        };
                        if let Some(ref q) = query {
                            let name = row.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            let ticker = row.get(2).and_then(|v| v.as_str()).unwrap_or("");
                            name.to_lowercase().contains(q)
                                || ticker.to_lowercase().contains(q)
                        } else {
                            true
                        }
                    })
                    .take(50)
                    .map(|row| {
                        let row = row.as_array().unwrap();
                        json!({
                            "cik": row.first().cloned().unwrap_or(Value::Null),
                            "name": row.get(1).cloned().unwrap_or(Value::Null),
                            "ticker": row.get(2).cloned().unwrap_or(Value::Null),
                            "exchange": row.get(3).cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect(),
                None => vec![],
            };

            let total = rows.map(|r| r.len()).unwrap_or(0);

            Ok(json!({
                "total_tickers": total,
                "returned": results.len(),
                "query": query.as_deref().unwrap_or("(none)"),
                "data": results
            }))
        }

        "sec_company_facts" => {
            let client = state.read().await.client()?;
            let ticker = args["ticker"]
                .as_str()
                .context("ticker required")?
                .to_string();
            let taxonomy_filter = args["taxonomy"].as_str().map(|s| s.to_string());
            let cik = client.cik_for_ticker(&ticker).await?;
            let data = client.company_facts(&cik).await?;

            let entity = data["entityName"].as_str().unwrap_or("Unknown").to_string();
            let facts = data["facts"].as_object();

            let mut taxonomies = json!({});

            if let Some(facts_map) = facts {
                for (tax_name, concepts_val) in facts_map {
                    if let Some(ref filter) = taxonomy_filter {
                        if tax_name != filter {
                            continue;
                        }
                    }
                    if let Some(concepts) = concepts_val.as_object() {
                        let concept_list: Vec<Value> = concepts
                            .iter()
                            .map(|(concept_name, concept_data)| {
                                json!({
                                    "concept": concept_name,
                                    "label": concept_data["label"].as_str().unwrap_or(concept_name),
                                    "description": concept_data["description"].as_str().unwrap_or(""),
                                })
                            })
                            .collect();
                        taxonomies[tax_name] = json!(concept_list);
                    }
                }
            }

            Ok(json!({
                "company": entity,
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "taxonomies": taxonomies
            }))
        }

        "sec_xbrl_frames" => {
            let client = state.read().await.client()?;
            let concept = args["concept"]
                .as_str()
                .context("concept required")?
                .to_string();
            let period_input = args["period"]
                .as_str()
                .context("period required")?
                .trim()
                .trim_start_matches("CY")
                .trim_start_matches("cy")
                .trim_end_matches('I')
                .trim_end_matches('i')
                .to_string();
            let instant = args["instant"].as_bool().unwrap_or(false);
            let taxonomy = args["taxonomy"].as_str().unwrap_or("us-gaap").to_string();
            let unit = args["unit"].as_str().unwrap_or("USD").to_string();
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;

            if instant && !period_input.contains('Q') {
                anyhow::bail!(
                    "instant=true requires a quarterly period (e.g. '2024Q1'); got '{period_input}'"
                );
            }

            let period_code = if instant {
                format!("{period_input}I")
            } else {
                period_input.clone()
            };

            let data = client
                .xbrl_frames(&taxonomy, &concept, &unit, &period_code)
                .await?;

            let mut entries: Vec<Value> = data["data"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            entries.sort_by(|a, b| {
                let va = a["val"].as_f64().unwrap_or(0.0);
                let vb = b["val"].as_f64().unwrap_or(0.0);
                vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
            });

            let top: Vec<Value> = entries.into_iter().take(limit).collect();

            Ok(json!({
                "concept": concept,
                "taxonomy": taxonomy,
                "unit": unit,
                "period": period_code,
                "instant": instant,
                "count": top.len(),
                "data": top
            }))
        }

        _ => anyhow::bail!("unknown tool: {}", name),
    }
}

// ── MCP Protocol types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Request {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl Response {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── MCP message dispatch ──────────────────────────────────────────────────────

async fn dispatch(state: &Arc<RwLock<State>>, req: Request) -> Option<Response> {
    let id = req.id.clone().unwrap_or(Value::Null);

    if req.id.is_none() && req.method.starts_with("notifications/") {
        return None;
    }

    let resp = match req.method.as_str() {
        "initialize" => {
            // Echo the client's requested protocol version when we support it;
            // otherwise fall back to our latest and let the client decide.
            let requested = req
                .params
                .as_ref()
                .and_then(|p| p["protocolVersion"].as_str());
            let negotiated = match requested {
                Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v,
                _ => MCP_PROTOCOL_VERSION,
            };
            if let Some(v) = requested {
                debug!("client requested protocol {v}, negotiated {negotiated}");
            }
            Response::ok(
                id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "sec-mcp", "version": SERVER_VERSION }
                }),
            )
        }

        "tools/list" => {
            let configured = state.read().await.is_configured();
            Response::ok(id, tool_list(configured))
        }

        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let tool_name = match params["name"].as_str() {
                Some(n) => n.to_string(),
                None => return Some(Response::err(id, -32602, "missing tool name")),
            };
            let args = params["arguments"].clone();

            debug!("calling tool: {tool_name}");

            match handle_tool(state, &tool_name, &args).await {
                Ok(result) => Response::ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": result.to_string() }]
                    }),
                ),
                Err(e) => {
                    error!("tool error: {e:#}");
                    Response::ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
                            "isError": true
                        }),
                    )
                }
            }
        }

        other => {
            debug!("unhandled method: {other}");
            Response::err(id, -32601, format!("method not found: {other}"))
        }
    };

    Some(resp)
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sec_mcp=info".parse().unwrap()),
        )
        .init();

    info!("SEC EDGAR MCP server starting");

    let state = Arc::new(RwLock::new(State::new()));

    {
        let s = state.read().await;
        if s.is_configured() {
            info!(
                "loaded contact email: {}",
                s.config.contact_email.as_deref().unwrap_or("")
            );
        } else {
            info!("no contact email configured — Claude will call sec_configure on first use");
        }
    }

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Some(line) = reader.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        debug!("← {line}");

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!("parse error: {e}");
                let resp = Response::err(Value::Null, -32700, format!("parse error: {e}"));
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        if let Some(resp) = dispatch(&state, req).await {
            let mut out = serde_json::to_string(&resp)?;
            out.push('\n');
            debug!("→ {out}");
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    info!("SEC EDGAR MCP server shutting down");
    Ok(())
}
