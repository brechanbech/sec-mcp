//! SEC EDGAR MCP Server
//!
//! Exposes SEC EDGAR API data as MCP tools for Claude Desktop.
//! Communicates over stdio using the MCP JSON-RPC protocol.
//!
//! On first use of any data tool, Claude will call `sec_configure` to ask the
//! user for their contact email. This is required by the SEC EDGAR fair-access
//! policy (https://www.sec.gov/os/accessing-edgar-data). The email is stored
//! in ~/.config/sec-mcp/config.toml and used as the HTTP User-Agent contact.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

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
}

impl EdgarClient {
    fn new(contact_email: &str) -> Result<Self> {
        let user_agent = format!("sec-mcp/0.1 (contact: {contact_email})");
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { http })
    }

    async fn cik_for_ticker(&self, ticker: &str) -> Result<String> {
        let url = "https://www.sec.gov/files/company_tickers.json";
        let map: HashMap<String, Value> = self.http.get(url).send().await?.json().await?;

        let ticker_upper = ticker.to_uppercase();
        for entry in map.values() {
            if let Some(t) = entry.get("ticker").and_then(|v| v.as_str()) {
                if t == ticker_upper {
                    if let Some(cik) = entry.get("cik_str") {
                        return Ok(format!("{:010}", cik.as_u64().unwrap_or(0)));
                    }
                }
            }
        }
        anyhow::bail!("ticker '{}' not found in EDGAR", ticker)
    }

    async fn recent_filings(&self, cik: &str) -> Result<Value> {
        let url = format!("https://data.sec.gov/submissions/CIK{cik}.json");
        let val: Value = self.http.get(&url).send().await?.json().await?;
        Ok(val)
    }

    async fn company_concept(&self, cik: &str, taxonomy: &str, concept: &str) -> Result<Value> {
        let url = format!(
            "https://data.sec.gov/api/xbrl/companyconcept/CIK{cik}/{taxonomy}/{concept}.json"
        );
        let val: Value = self.http.get(&url).send().await?.json().await?;
        Ok(val)
    }
}

// ── Shared server state ───────────────────────────────────────────────────────

struct State {
    config: Config,
    client: Option<EdgarClient>,
}

impl State {
    fn new() -> Self {
        let config = Config::load();
        let client = config
            .contact_email
            .as_deref()
            .and_then(|email| EdgarClient::new(email).ok());
        Self { config, client }
    }

    fn is_configured(&self) -> bool {
        self.config.contact_email.is_some()
    }

    fn set_email(&mut self, email: String) -> Result<()> {
        self.client = Some(EdgarClient::new(&email)?);
        self.config.contact_email = Some(email);
        self.config.save()?;
        Ok(())
    }

    fn client(&self) -> Result<&EdgarClient> {
        self.client.as_ref().context(
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
                "description": "Get recent SEC filings (10-K, 10-Q, 8-K, etc.) for a company by ticker symbol.",
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
            }
        ]
    })
}

// ── Tool handlers ─────────────────────────────────────────────────────────────

async fn handle_tool(state: &Arc<RwLock<State>>, name: &str, args: &Value) -> Result<Value> {
    match name {
        "sec_configure" => {
            let email = args["contact_email"]
                .as_str()
                .context("contact_email required")?
                .trim()
                .to_string();

            if !email.contains('@') || !email.contains('.') {
                anyhow::bail!("'{}' does not look like a valid email address", email);
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
                     All requests will now identify as: sec-mcp/0.1 (contact: {email})"
                )
            }))
        }

        "sec_lookup_cik" => {
            let s = state.read().await;
            let client = s.client()?;
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
            let ticker;
            let form_filter_owned;
            let limit;
            let cik;
            let data;
            {
                let s = state.read().await;
                let client = s.client()?;
                ticker = args["ticker"]
                    .as_str()
                    .context("ticker required")?
                    .to_string();
                form_filter_owned = args["form_type"].as_str().map(|s| s.to_string());
                limit = args["limit"].as_u64().unwrap_or(10).min(40) as usize;
                cik = client.cik_for_ticker(&ticker).await?;
                data = client.recent_filings(&cik).await?;
            }

            let form_filter = form_filter_owned.as_deref();
            let company_name = data["name"].as_str().unwrap_or("Unknown").to_string();
            let recent = &data["filings"]["recent"];

            let forms = recent["form"].as_array().cloned().unwrap_or_default();
            let dates = recent["filingDate"].as_array().cloned().unwrap_or_default();
            let accs = recent["accessionNumber"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let docs = recent["primaryDocument"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let descs = recent["primaryDocDescription"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            let cik_num = cik.trim_start_matches('0').to_string();

            let filings: Vec<Value> = forms
                .iter()
                .enumerate()
                .filter(|(_, f)| form_filter.map_or(true, |ft| f.as_str().unwrap_or("") == ft))
                .take(limit)
                .map(|(i, f)| {
                    let acc_raw = accs.get(i).and_then(|v| v.as_str()).unwrap_or("");
                    let acc_nodash = acc_raw.replace('-', "");
                    let doc = docs.get(i).and_then(|v| v.as_str()).unwrap_or("");
                    let url = format!(
                        "https://www.sec.gov/Archives/edgar/data/{cik_num}/{acc_nodash}/{doc}"
                    );
                    json!({
                        "form": f,
                        "date": dates.get(i).cloned().unwrap_or(Value::Null),
                        "description": descs.get(i).cloned().unwrap_or(Value::Null),
                        "accession_number": acc_raw,
                        "url": url
                    })
                })
                .collect();

            Ok(json!({
                "company": company_name,
                "ticker": ticker.to_uppercase(),
                "cik": cik,
                "filings": filings
            }))
        }

        "sec_financial_concept" => {
            let ticker;
            let concept;
            let taxonomy;
            let period_filter_owned;
            let cik;
            let data;
            {
                let s = state.read().await;
                let client = s.client()?;
                ticker = args["ticker"]
                    .as_str()
                    .context("ticker required")?
                    .to_string();
                concept = args["concept"]
                    .as_str()
                    .context("concept required")?
                    .to_string();
                taxonomy = args["taxonomy"].as_str().unwrap_or("us-gaap").to_string();
                period_filter_owned = args["period"].as_str().map(|s| s.to_string());
                cik = client.cik_for_ticker(&ticker).await?;
                data = client.company_concept(&cik, &taxonomy, &concept).await?;
            }

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
            let ticker;
            let cik;
            let data;
            {
                let s = state.read().await;
                let client = s.client()?;
                ticker = args["ticker"]
                    .as_str()
                    .context("ticker required")?
                    .to_string();
                cik = client.cik_for_ticker(&ticker).await?;
                data = client.recent_filings(&cik).await?;
            }

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
        "initialize" => Response::ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "sec-mcp", "version": "0.1.0" }
            }),
        ),

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
