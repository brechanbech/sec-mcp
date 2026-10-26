//! Protocol and opt-in live smoke tests.
//!
//! Both drive the built `sec-mcp` binary over stdio (JSON-RPC, exactly as an MCP
//! client would), so they exercise the real `rmcp` protocol layer rather than
//! calling into the crate.
//!
//! [`protocol_surface`] runs **always** and touches no network: it covers the
//! MCP `2026-07-28` surface — `server/discover`, version negotiation, the
//! `tools/list` cache hints, and the stateless (handshake-free) request path.
//!
//! [`live_smoke`] hits the **real** SEC EDGAR APIs and is **skipped by
//! default** — it runs only when `SEC_MCP_LIVE_EMAIL` is set, which both opts in
//! and supplies the contact email EDGAR's fair-access policy requires:
//!
//! ```sh
//! SEC_MCP_LIVE_EMAIL="you@example.com" cargo test --test live_smoke -- --nocapture
//! ```
//!
//! Purpose: catch EDGAR (or our URL construction) drifting under us — the thing
//! no offline test can see. In particular `frames_per_share_unit_returns_data`
//! is a regression guard for the compound-unit bug (`USD/shares` must reach the
//! frames API as `USD-per-shares`, not a percent-encoded slash) fixed in 0.4.2.
//!
//! Everything runs inside one `#[test]` against a single server process: the
//! calls share one throttled `EdgarClient` and go out sequentially, so the suite
//! stays well within EDGAR's rate limit and won't hammer the SEC.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

/// Distinguishes concurrent [`Server`] sandboxes. Tests share one process, so
/// the pid alone is not unique enough once there is more than one test.
static SANDBOX_SEQ: AtomicU32 = AtomicU32::new(0);

/// The `initialize` params every MCP revision requires. `capabilities` and
/// `clientInfo` are mandatory alongside `protocolVersion`; omitting them leaves
/// the handshake incomplete and the server will not serve.
fn init_params(protocol_version: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {},
        "clientInfo": { "name": "sec-mcp-tests", "version": "0" }
    })
}

/// The self-describing `_meta` a `2026-07-28` client puts on every request when
/// it skips the handshake. `protocolVersion` and `clientCapabilities` are both
/// required; a request missing either is rejected with `-32602`.
fn meta_block() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "sec-mcp-tests", "version": "0" }
    })
}

/// The opt-in switch and contact email, read once. Absence skips the suite.
fn live_email() -> Option<String> {
    std::env::var("SEC_MCP_LIVE_EMAIL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A spawned `sec-mcp` server we speak line-delimited JSON-RPC to.
///
/// `HOME`/`XDG_CONFIG_HOME` are redirected to a throwaway directory so
/// `sec_configure` can't touch the developer's real `config.toml`, and so the
/// server starts unconfigured for a deterministic run.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    sandbox: PathBuf,
    next_id: i64,
}

impl Server {
    fn start() -> Self {
        let seq = SANDBOX_SEQ.fetch_add(1, Ordering::Relaxed);
        let sandbox =
            std::env::temp_dir().join(format!("sec-mcp-live-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&sandbox).expect("create sandbox config dir");

        let mut child = Command::new(env!("CARGO_BIN_EXE_sec-mcp"))
            .env("HOME", &sandbox)
            .env("XDG_CONFIG_HOME", &sandbox)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sec-mcp binary");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Server {
            child,
            stdin,
            stdout,
            sandbox,
            next_id: 1,
        }
    }

    /// Send one request and return the response whose `id` matches.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{req}").expect("write request");
        self.stdin.flush().expect("flush request");

        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).expect("read response");
            assert!(n > 0, "server closed stdout before answering id {id}");
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return v;
            }
        }
    }

    /// Send a notification (no `id`, so no response is expected).
    fn notify(&mut self, method: &str) {
        let req = json!({ "jsonrpc": "2.0", "method": method });
        writeln!(self.stdin, "{req}").expect("write notification");
        self.stdin.flush().expect("flush notification");
    }

    /// Complete the MCP handshake and return the negotiated protocol version.
    fn handshake(&mut self, protocol_version: &str) -> String {
        let init = self.request("initialize", init_params(protocol_version));
        assert_eq!(
            init["result"]["serverInfo"]["name"], "sec-mcp",
            "init: {init}"
        );
        self.notify("notifications/initialized");
        init["result"]["protocolVersion"]
            .as_str()
            .unwrap_or_else(|| panic!("init: no negotiated protocolVersion in {init}"))
            .to_string()
    }

    /// Call a tool and return its result payload (the tool's JSON, already parsed
    /// out of the MCP text-content envelope). Panics if the tool reports an error.
    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let result = &resp["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool {name}: no text content in {resp}"));
        assert!(
            result["isError"].as_bool() != Some(true),
            "tool {name} returned an error: {text}"
        );
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text }))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.sandbox);
    }
}

#[test]
fn live_smoke() {
    let Some(email) = live_email() else {
        eprintln!("SEC_MCP_LIVE_EMAIL not set — skipping live smoke tests");
        return;
    };

    let mut server = Server::start();

    // Handshake, then register the contact email every data tool needs.
    server.handshake("2025-06-18");
    server.call_tool("sec_configure", json!({ "contact_email": email }));

    // Ticker → CIK: the resolved form is 10-digit, zero-padded, no prefix.
    let cik = server.call_tool("sec_lookup_cik", json!({ "ticker": "AAPL" }));
    assert_eq!(cik["cik"].as_str(), Some("0000320193"), "AAPL CIK: {cik}");

    // A company with no current ticker is reachable by CIK. US Steel left SEC's
    // ticker list when Nippon Steel took it private, but its filings stay in
    // EDGAR; the October 2026 benchmark lost three questions to this.
    let by_cik = server.call_tool("sec_lookup_cik", json!({ "ticker": "1163302" }));
    assert!(
        by_cik["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("steel"),
        "CIK 1163302 should be US Steel: {by_cik}"
    );

    // A filer with no ticker is reachable by name, through EDGAR's undocumented
    // company autocomplete. Bridgewater files 13Fs but has no ticker.
    let by_name = server.call_tool(
        "sec_lookup_cik",
        json!({ "name": "Bridgewater Associates" }),
    );
    assert_eq!(
        by_name["matches"][0]["cik"].as_str(),
        Some("0001350694"),
        "Bridgewater Associates, LP should rank first: {by_name}"
    );
    // A listed company's match carries its tickers, split out of the name.
    let apple = server.call_tool("sec_lookup_cik", json!({ "name": "Apple" }));
    assert_eq!(
        apple["matches"][0]["cik"].as_str(),
        Some("0000320193"),
        "{apple}"
    );
    assert_eq!(apple["matches"][0]["tickers"], json!(["AAPL"]), "{apple}");
    assert_eq!(
        apple["matches"][0]["name"].as_str(),
        Some("Apple Inc."),
        "{apple}"
    );
    let delisted = server.call_tool(
        "sec_financial_concept",
        json!({ "ticker": "1163302", "concept": "InventoryNet", "period": "annual", "limit": 1 }),
    );
    assert!(
        delisted["data"][0]["val"].is_number(),
        "inventory by CIK: {delisted}"
    );

    // Company facts: the hostile XBRL shape decodes and carries us-gaap concepts.
    let facts = server.call_tool("sec_company_facts", json!({ "ticker": "AAPL" }));
    assert!(
        facts["company"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("apple"),
        "company facts entity: {facts}"
    );
    let concepts = facts["concepts"].as_array().expect("concepts array");
    assert!(
        concepts.iter().any(|c| c["taxonomy"] == "us-gaap"),
        "expected us-gaap concepts: {facts}"
    );

    // Paged and slim: the unfiltered listing ran to 181k characters for Netflix,
    // past what a client will pass on. A default page stays well under that.
    let nflx = server.call_tool("sec_company_facts", json!({ "ticker": "NFLX" }));
    let size = nflx.to_string().len();
    assert!(size < 40_000, "NFLX facts page is {size} characters");
    assert!(
        nflx["next_offset"].is_number(),
        "NFLX has more than one page: matched {}",
        nflx["matched"]
    );
    let revenue = server.call_tool(
        "sec_company_facts",
        json!({ "ticker": "NFLX", "query": "revenue" }),
    );
    assert!(
        revenue["concepts"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(|c| c["latest"].is_string())),
        "revenue concepts with a latest date: {revenue}"
    );

    // Recent filings, filtered to 10-K: the column-major block zips into rows.
    let filings = server.call_tool(
        "sec_recent_filings",
        json!({ "ticker": "AAPL", "form_type": "10-K", "limit": 3 }),
    );
    let rows = filings["filings"].as_array().expect("filings array");
    assert!(!rows.is_empty(), "expected at least one 10-K: {filings}");
    assert_eq!(
        rows[0]["form"].as_str(),
        Some("10-K"),
        "first row form: {filings}"
    );

    // Latest filings across EDGAR, from the undocumented latest-filings feed.
    // A Form 4 is listed once per party, so grouping must give each filing both
    // its reporting person and its issuer.
    let latest = server.call_tool(
        "sec_recent_filings",
        json!({ "form_type": "4", "limit": 10 }),
    );
    let latest_rows = latest["filings"].as_array().expect("latest filings array");
    assert_eq!(latest_rows.len(), 10, "ten latest Form 4s: {latest}");
    assert!(
        latest_rows.iter().all(|f| f["accession"].is_string()),
        "every latest filing has an accession: {latest}"
    );
    assert!(
        latest_rows.iter().any(|f| {
            let roles: Vec<&str> = f["parties"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["role"].as_str())
                .collect();
            roles.contains(&"Reporting") && roles.contains(&"Issuer")
        }),
        "a Form 4 groups its reporting person and issuer: {latest}"
    );

    // Full-text search with no query lists filings by form and date, one per
    // filing.
    let listed = server.call_tool(
        "sec_full_text_search",
        json!({
            "forms": "10-K",
            "start_date": "2025-02-10",
            "end_date": "2025-02-14",
            "limit": 20
        }),
    );
    let listed_rows = listed["results"].as_array().expect("results array");
    let mut accessions: Vec<&str> = listed_rows
        .iter()
        .filter_map(|r| r["accession"].as_str())
        .collect();
    let returned = accessions.len();
    accessions.sort_unstable();
    accessions.dedup();
    assert!(
        returned == 20 && accessions.len() == returned,
        "20 distinct 10-Ks filed 10–14 Feb 2025: {listed}"
    );

    // Foreign private issuer — the 0.5.0 regression guard. XBRL is not
    // US-domestic-only: Novartis reports `ifrs-full` facts on 6-K, which the
    // old `form == "10-Q"` filter excluded, so `quarterly` came back empty and
    // read as "this company reports nothing".
    let fpi = server.call_tool(
        "sec_financial_concept",
        json!({
            "ticker": "NVS",
            "concept": "AccountingProfit",
            "taxonomy": "ifrs-full",
            "period": "quarterly"
        }),
    );
    let fpi_rows = fpi["data"].as_array().expect("fpi data array");
    assert!(
        !fpi_rows.is_empty(),
        "foreign filer's quarterly facts came back empty — the 6-K form mapping is gone: {fpi}"
    );

    // The same concept has no *annual* facts, and an empty result must say why
    // rather than look like missing data.
    let fpi_annual = server.call_tool(
        "sec_financial_concept",
        json!({
            "ticker": "NVS",
            "concept": "AccountingProfit",
            "taxonomy": "ifrs-full",
            "period": "annual"
        }),
    );
    assert!(
        fpi_annual["available_forms"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "an empty period filter must report the forms actually present: {fpi_annual}"
    );

    // Amendments on request. Apple's January 2010 10-K/A restated fiscal 2009
    // under new revenue-recognition rules: left out of 'annual' by default,
    // there when asked for.
    let mut restated = |include: bool| {
        server.call_tool(
            "sec_financial_concept",
            json!({
                "ticker": "AAPL",
                "concept": "SalesRevenueNet",
                "period": "annual",
                "include_amendments": include,
                "limit": 100
            }),
        )
    };
    let with = restated(true);
    let without = restated(false);
    let has_amendment = |v: &serde_json::Value| {
        v["data"]
            .as_array()
            .is_some_and(|d| d.iter().any(|f| f["form"] == "10-K/A"))
    };
    assert!(
        has_amendment(&with) && with["amended_facts"].as_u64() > Some(0),
        "Apple's 10-K/A facts with include_amendments: {with}"
    );
    assert!(
        !has_amendment(&without),
        "no 10-K/A facts by default: {without}"
    );

    // Paging back through a concept: page two must reach further into the past
    // than page one. "Past" means period `end`, which EDGAR sorts on; `filed`
    // is not monotonic, since each 10-K re-reports earlier years as
    // comparatives. Apple has filed NetIncomeLoss for well over a decade.
    let concept_page = |offset: u64| {
        json!({ "ticker": "AAPL", "concept": "NetIncomeLoss", "period": "annual",
                "limit": 5, "offset": offset })
    };
    let newest = server.call_tool("sec_financial_concept", concept_page(0));
    assert_eq!(
        newest["next_offset"],
        json!(5),
        "page one must point at page two: {newest}"
    );
    let older = server.call_tool("sec_financial_concept", concept_page(5));
    let period_end =
        |page: &Value, idx: usize| page["data"][idx]["end"].as_str().map(str::to_owned);
    assert_eq!(
        older["returned"],
        json!(5),
        "page two came back short: {older}"
    );
    assert!(
        period_end(&older, 4) <= period_end(&newest, 0),
        "page two should reach back before page one: {older} vs {newest}"
    );

    // Paging through tickers: the second page continues where the first ended.
    let tickers_page = |offset: u64| json!({ "query": "bank", "limit": 5, "offset": offset });
    let first_page = server.call_tool("sec_list_tickers", tickers_page(0));
    assert!(
        first_page["matched"].as_u64().is_some_and(|m| m > 5),
        "'bank' should match more than one page of tickers: {first_page}"
    );
    assert_eq!(first_page["next_offset"], json!(5), "{first_page}");
    let second_page = server.call_tool("sec_list_tickers", tickers_page(5));
    assert_ne!(
        first_page["data"][0]["ticker"], second_page["data"][0]["ticker"],
        "page two repeated page one: {second_page}"
    );

    // Insider transactions: the Form 4 XML is fetched and parsed, not just
    // linked. Guards the URL rule (submissions points at the XSL-rendered path;
    // the raw document is that path with the `xsl…/` segment stripped) and the
    // transaction-code expansion.
    let insider = server.call_tool(
        "sec_insider_transactions",
        json!({ "ticker": "AAPL", "form_type": "4", "limit": 2 }),
    );
    let insider_filings = insider["filings"].as_array().expect("filings array");
    assert!(
        !insider_filings.is_empty(),
        "expected recent AAPL Form 4s: {insider}"
    );
    let first = &insider_filings[0];
    assert!(
        first["error"].is_null(),
        "could not read the Form 4 document — the xsl-strip URL rule may be broken: {first}"
    );
    assert!(
        first["insiders"][0]["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty()),
        "expected a named reporting owner: {first}"
    );
    let txns = first["transactions"]
        .as_array()
        .expect("transactions array");
    assert!(!txns.is_empty(), "expected Form 4 transactions: {first}");
    assert!(
        txns.iter().all(|t| t["code"].as_str().is_some()),
        "every transaction needs a code: {first}"
    );

    // Paging back: `next_offset` continues with older filings, not the same ones.
    assert_eq!(insider["next_offset"], json!(2), "{insider}");
    let older_insider = server.call_tool(
        "sec_insider_transactions",
        json!({ "ticker": "AAPL", "form_type": "4", "limit": 2, "offset": 2 }),
    );
    assert_ne!(
        older_insider["filings"][0]["accession"], first["accession"],
        "offset 2 repeated the first filing: {older_insider}"
    );

    // Form 3 reports holdings, not transactions. An empty `transactions` array
    // must not mean an empty result — the same silent-empty failure fixed in
    // the period filter. A new officer who owns nothing yet legitimately files
    // empty tables, but then the document must say so via the flag.
    let form3 = server.call_tool(
        "sec_insider_transactions",
        json!({ "ticker": "NVDA", "form_type": "3", "limit": 1 }),
    );
    if let Some(f) = form3["filings"].as_array().and_then(|a| a.first()) {
        assert!(
            f["holdings"].as_array().is_some_and(|h| !h.is_empty())
                || f["no_securities_owned"] == json!(true),
            "a Form 3 must report holdings or say none are owned, not nothing: {f}"
        );
    }

    // Form 13F: Berkshire's holdings report, aggregated from its table rows (one
    // per sub-manager) into positions that add up to the cover page's total.
    let brk = server.call_tool(
        "sec_institutional_holdings",
        json!({ "ticker": "BRK-B", "query": "apple" }),
    );
    assert_eq!(brk["form"], "13F-HR", "Berkshire 13F: {brk}");
    assert!(
        brk["holdings"][0]["issuer"]
            .as_str()
            .is_some_and(|i| i.contains("APPLE"))
            && brk["holdings"][0]["value"].as_i64() > Some(0),
        "Berkshire holds Apple: {brk}"
    );
    assert_eq!(
        brk["portfolio_value"], brk["cover"]["table_value_total"],
        "positions add up to the cover page total: {brk}"
    );

    // Form 144: Apple's officers file notices of proposed sales, usually
    // several a month, each naming the seller and the shares to be sold.
    let notices = server.call_tool(
        "sec_insider_transactions",
        json!({ "ticker": "AAPL", "form_type": "144", "limit": 2 }),
    );
    let notice = &notices["filings"][0];
    assert_eq!(notice["form"], "144", "AAPL Form 144: {notices}");
    assert!(
        notice["seller"]["name"].is_string() && notice["proposed_sales"][0]["shares"].is_number(),
        "a Form 144 names its seller and the shares to be sold: {notice}"
    );

    // Form D. SpaceX raised privately for twenty years before listing, and its
    // last Form D (August 2022) reports $250M sold under Rule 506(b). Taken by
    // CIK, as most Form D filers have no ticker.
    let spacex = server.call_tool(
        "sec_private_offerings",
        json!({ "ticker": "1181412", "limit": 1 }),
    );
    let offering = &spacex["filings"][0];
    assert!(
        offering["total_amount_sold"].is_number() && offering["exemptions"][0]["code"].is_string(),
        "a Form D reports the amount sold and its exemption: {spacex}"
    );
    // A fund's offering is open-ended, and its type is reported.
    let fund = server.call_tool(
        "sec_private_offerings",
        json!({ "ticker": "1507589", "limit": 1 }),
    );
    let fund_offering = &fund["filings"][0];
    assert_eq!(
        fund_offering["industry"], "Pooled Investment Fund",
        "Bridgewater Pure Alpha Fund VI: {fund}"
    );
    assert!(fund_offering["fund"]["type"].is_string(), "{fund_offering}");

    // Schedules 13D and 13G. Apple's holders file 13Gs: the newest is XML with
    // a reporting person who owns a percentage of the class.
    let holders = server.call_tool(
        "sec_beneficial_ownership",
        json!({ "ticker": "AAPL", "form_type": "13G", "limit": 3 }),
    );
    let first = &holders["filings"][0];
    assert_eq!(first["schedule"], "13G", "AAPL 13G: {holders}");
    assert_eq!(first["issuer"]["cik"], "0000320193", "{first}");
    assert!(
        first["reporting_persons"][0]["percent_of_class"].is_number(),
        "a 13G reporting person has a percent of class: {first}"
    );
    // A 13D carries its purpose. Coliseum's August 2026 13D on Sonos replaced
    // its 13G; read through the holder's own CIK, the other side of the index.
    let coliseum = server.call_tool(
        "sec_beneficial_ownership",
        json!({ "ticker": "1409751", "form_type": "13D", "limit": 10 }),
    );
    let sonos = coliseum["filings"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["issuer"]["cik"] == "0001314727"))
        .unwrap_or_else(|| panic!("Coliseum's 13D on Sonos: {coliseum}"));
    assert!(
        sonos["purpose"].as_str().is_some_and(|p| !p.is_empty()),
        "a 13D carries Item 4: {sonos}"
    );

    // Cross-company frame on a COMPOUND unit — the 0.4.2 regression guard. A
    // per-share concept must reach the API as `.../USD-per-shares/...`; the old
    // percent-encoded `USD%2Fshares` 404'd and this came back empty.
    let eps = server.call_tool(
        "sec_xbrl_frames",
        json!({
            "concept": "EarningsPerShareBasic",
            "period": "2019",
            "unit": "USD/shares",
            "instant": false,
            "limit": 5
        }),
    );
    let eps_rows = eps["data"].as_array().expect("frames data array");
    assert!(
        !eps_rows.is_empty(),
        "per-share frame returned no rows — the compound-unit bug is back: {eps}"
    );

    // Cross-company frame on a SIMPLE unit / instant concept — the common path.
    let assets = server.call_tool(
        "sec_xbrl_frames",
        json!({
            "concept": "Assets",
            "period": "2019Q1",
            "unit": "USD",
            "instant": true,
            "limit": 5
        }),
    );
    assert!(
        assets["data"].as_array().is_some_and(|a| !a.is_empty()),
        "instant frame returned no rows: {assets}"
    );
    // Thousands of filers report Assets; the page says so and pages on.
    assert!(
        assets["total"].as_u64().is_some_and(|t| t > 5) && assets["next_offset"] == json!(5),
        "frame paging fields: total {} next_offset {}",
        assets["total"],
        assets["next_offset"]
    );

    // Filing text. The two answers Claude Desktop's own fetch could not reach in
    // October 2026: guidance in an 8-K's exhibit, and a sentence ~170k
    // characters into a 10-K. First, a filing with no document named lists its
    // documents by type, which is how the exhibit is found at all.
    let eight_k = server.call_tool(
        "sec_filing_text",
        json!({ "url": "https://www.sec.gov/Archives/edgar/data/1559720/000119312525109934/" }),
    );
    assert_eq!(eight_k["form"], "8-K", "{eight_k}");
    let exhibit_url = eight_k["documents"]
        .as_array()
        .and_then(|docs| docs.iter().find(|d| d["type"] == "EX-99.1"))
        .and_then(|d| d["url"].as_str())
        .unwrap_or_else(|| panic!("no EX-99.1 in the 8-K's documents: {eight_k}"))
        .to_string();
    let letter = server.call_tool(
        "sec_filing_text",
        json!({ "url": exhibit_url, "find": "expect to generate revenue" }),
    );
    let guidance = letter["matches"][0]["text"].as_str().unwrap_or("");
    assert!(
        guidance.contains("$2.99 billion to $3.05 billion"),
        "Q2 2025 guidance in the shareholder letter: {letter}"
    );
    assert!(
        letter["images"].as_u64().is_some_and(|n| n > 0),
        "the letter's page images should be counted: {letter}"
    );

    let ten_k_url =
        "https://www.sec.gov/Archives/edgar/data/1559720/000155972025000010/abnb-20241231.htm";
    let first_page = server.call_tool("sec_filing_text", json!({ "url": ten_k_url }));
    assert!(
        first_page["text"]
            .as_str()
            .is_some_and(|t| t.len() <= 20_000),
        "default page size: {}",
        first_page["returned"]
    );
    let labels: Vec<&str> = first_page["sections"]
        .as_array()
        .map(|s| s.iter().filter_map(|x| x["label"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        labels.contains(&"Part I, Item 1A") && labels.contains(&"Part II, Item 7"),
        "10-K section index: {labels:?}"
    );
    let risk = server.call_tool(
        "sec_filing_text",
        json!({ "url": ten_k_url, "section": "1A", "limit": 1000 }),
    );
    assert!(
        risk["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("Item 1A")),
        "section 1A should start at its heading: {}",
        risk["text"]
    );
    let apac = server.call_tool(
        "sec_filing_text",
        json!({ "url": ten_k_url, "find": "3.3 for Asia Pacific" }),
    );
    assert!(
        apac["total_matches"].as_u64().is_some_and(|n| n >= 1),
        "nights per booking by region: {apac}"
    );

    // Full-text search. This is the one endpoint sec-mcp calls that SEC does not
    // document — `efts.sec.gov`, the API behind the EDGAR search UI — so it has
    // no stability guarantee and this assertion is the only thing that would
    // notice it drifting. Deliberately scoped to one company so the result set
    // is small and the assertions can be specific.
    let hits = server.call_tool(
        "sec_full_text_search",
        json!({ "query": "\"iPhone\"", "ticker": "AAPL", "forms": "10-K", "limit": 3 }),
    );
    let results = hits["results"].as_array().expect("results array");
    assert!(
        !results.is_empty(),
        "full-text search returned nothing for a phrase Apple certainly filed: {hits}"
    );

    let first = &results[0];
    assert_eq!(
        first["cik"].as_str(),
        Some("0000320193"),
        "the ticker filter should have scoped this to Apple: {hits}"
    );
    // EFTS matches `forms` against `root_forms`, so it returns `10-K/A` for
    // `10-K` — first-ranked here on 23 October 2026. The tool drops those by
    // default; a leaked amendment means that filtering broke.
    assert!(
        results.iter().all(|r| r["form"].as_str() == Some("10-K")),
        "form filter let a non-10-K through: {hits}"
    );
    assert!(
        hits["next_offset"].as_u64().is_some(),
        "23 matches and limit 3 must leave a next page: {hits}"
    );

    // And back in on request. Apple's January 2010 10-K/A (the FY2009
    // restatement) is historical, so it will always be in this result set.
    let with_amendments = server.call_tool(
        "sec_full_text_search",
        json!({ "query": "\"iPhone\"", "ticker": "AAPL", "forms": "10-K",
                "include_amendments": true, "limit": 100 }),
    );
    assert!(
        with_amendments["results"].as_array().is_some_and(|r| r
            .iter()
            .any(|h| h["form"] == "10-K/A" && h["amendment"] == json!(true))),
        "include_amendments should return the flagged FY2009 10-K/A: {with_amendments}"
    );

    // The document URL is the point of the tool — a hit whose URL cannot be
    // built is reported as `url_unavailable`, which means the id shape changed.
    let url = first["url"]
        .as_str()
        .unwrap_or_else(|| panic!("hit carried no URL (shape change?): {hits}"));
    assert!(
        url.starts_with("https://www.sec.gov/Archives/edgar/data/320193/"),
        "URL should address Apple's archive without zero-padding: {url}"
    );
    assert!(
        first["accession"].as_str().is_some_and(|a| a.contains('-')),
        "accession should keep its dashes: {hits}"
    );
}

/// The MCP `2026-07-28` surface. Offline: every assertion below is answered by
/// the protocol layer without touching EDGAR, so this runs on every `cargo test`
/// and is the regression guard for the port off the hand-rolled JSON-RPC layer.
#[test]
fn protocol_surface() {
    // ── Negotiation ──────────────────────────────────────────────────────────
    // A client naming 2026-07-28 over `initialize` is contradicting itself: that
    // revision replaced the handshake with the per-request `_meta` exercised
    // below, so there is no handshake in which to agree on it. The correct
    // answer is the server's newest revision that *does* have one. rmcp 3.0
    // echoed 2026-07-28 back instead, and this test asserted that; rmcp 3.4
    // fixed the negotiation, so the expectation moves with it rather than the
    // server having regressed.
    let mut server = Server::start();
    assert_eq!(server.handshake("2026-07-28"), "2025-11-25");
    drop(server);

    let mut server = Server::start();
    assert_eq!(server.handshake("2025-11-25"), "2025-11-25");

    // `resultType` is a 2026-07-28 field: it must be absent for an older peer,
    // which is what tells us rmcp is version-gating results rather than always
    // emitting them.
    let listed = server.request("tools/list", json!({}));
    assert!(
        listed["result"]["resultType"].is_null(),
        "resultType must be omitted for a 2025-11-25 peer: {listed}"
    );
    drop(server);

    // ── Stateless path ───────────────────────────────────────────────────────
    // 2026-07-28 drops the handshake: a request carrying its own `_meta` is a
    // valid opener. `server/discover` is mandatory in this revision — it was a
    // `-32601 method not found` before the port.
    let meta = json!({ "_meta": meta_block() });

    let mut server = Server::start();
    let discover = server.request("server/discover", meta.clone());
    assert!(
        discover["error"].is_null(),
        "server/discover must be implemented: {discover}"
    );
    let versions = discover["result"]["supportedVersions"]
        .as_array()
        .unwrap_or_else(|| panic!("discover: no supportedVersions in {discover}"));
    assert!(
        versions.iter().any(|v| v == "2026-07-28"),
        "discover must advertise 2026-07-28: {discover}"
    );
    // Identity comes from this crate, not the SDK — `Implementation::from_build_env`
    // would name rmcp here.
    assert_eq!(
        discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "sec-mcp",
        "discover serverInfo: {discover}"
    );

    // ── tools/list on the stateless path ─────────────────────────────────────
    let listed = server.request("tools/list", meta);
    let result = &listed["result"];
    assert_eq!(result["resultType"], "complete", "tools/list: {listed}");
    // SEP-2549 cache hints. `private`, not `public`: the listing reflects this
    // machine's configuration state.
    assert!(
        result["ttlMs"].as_u64().is_some_and(|t| t > 0),
        "tools/list must carry a positive ttlMs: {listed}"
    );
    assert_eq!(result["cacheScope"], "private", "tools/list: {listed}");

    let tools = result["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 14, "expected 14 tools: {listed}");

    // Named rather than merely counted: a count catches a tool vanishing but not
    // one being renamed, and the name is the part clients bind to.
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "sec_beneficial_ownership",
        "sec_company_facts",
        "sec_company_info",
        "sec_configure",
        "sec_filing_text",
        "sec_financial_concept",
        "sec_full_text_search",
        "sec_insider_transactions",
        "sec_institutional_holdings",
        "sec_list_tickers",
        "sec_lookup_cik",
        "sec_private_offerings",
        "sec_recent_filings",
        "sec_xbrl_frames",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    // Titles and behaviour hints on the wire, not just in the router: directories
    // grade tools on them, and the Claude directory requires `readOnlyHint` or
    // `destructiveHint` on every tool.
    for tool in tools {
        let name = &tool["name"];
        assert!(tool["title"].is_string(), "{name} has no title: {tool}");
        assert!(
            tool["annotations"]["readOnlyHint"].is_boolean(),
            "{name} has no readOnlyHint: {tool}"
        );
        assert!(
            tool["annotations"]["openWorldHint"].is_boolean(),
            "{name} has no openWorldHint: {tool}"
        );
    }

    // The server sandbox starts unconfigured, so `sec_configure` must carry the
    // first-run wording rather than the steady-state text.
    let configure = tools
        .iter()
        .find(|t| t["name"] == "sec_configure")
        .unwrap_or_else(|| panic!("sec_configure missing: {listed}"));
    assert!(
        configure["description"]
            .as_str()
            .is_some_and(|d| d.starts_with("REQUIRED SETUP")),
        "unconfigured sec_configure description: {configure}"
    );

    // ── Unconfigured guard ───────────────────────────────────────────────────
    // A data tool without a contact email fails as a *successful* result marked
    // `isError`, not a JSON-RPC error, so the model can read it and recover.
    // `_meta` rides on *every* request once the session is on the stateless
    // path — there is no handshake to carry it.
    let mut call = json!({ "name": "sec_lookup_cik", "arguments": { "ticker": "AAPL" } });
    call["_meta"] = meta_block();
    let resp = server.request("tools/call", call);
    assert_eq!(resp["result"]["isError"], true, "expected isError: {resp}");
    let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("not configured"),
        "expected the unconfigured guard, got: {text}"
    );
}
