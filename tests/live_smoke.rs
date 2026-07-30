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
    let usgaap = &facts["taxonomies"]["us-gaap"];
    assert!(
        usgaap.as_array().is_some_and(|a| !a.is_empty()),
        "expected non-empty us-gaap concepts: {facts}"
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

    // Form 3 reports holdings, not transactions. An empty `transactions` array
    // must not mean an empty result — the same silent-empty failure fixed in
    // the period filter.
    let form3 = server.call_tool(
        "sec_insider_transactions",
        json!({ "ticker": "NVDA", "form_type": "3", "limit": 1 }),
    );
    if let Some(f) = form3["filings"].as_array().and_then(|a| a.first()) {
        assert!(
            f["holdings"].as_array().is_some_and(|h| !h.is_empty()),
            "a Form 3 must report holdings rather than nothing: {f}"
        );
    }

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
}

/// The MCP `2026-07-28` surface. Offline: every assertion below is answered by
/// the protocol layer without touching EDGAR, so this runs on every `cargo test`
/// and is the regression guard for the port off the hand-rolled JSON-RPC layer.
#[test]
fn protocol_surface() {
    // ── Negotiation ──────────────────────────────────────────────────────────
    // The server offers 2026-07-28 and still meets older clients where they are.
    let mut server = Server::start();
    assert_eq!(server.handshake("2026-07-28"), "2026-07-28");
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
    assert_eq!(tools.len(), 9, "expected 9 tools: {listed}");

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
