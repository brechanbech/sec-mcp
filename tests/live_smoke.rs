//! Opt-in live smoke tests.
//!
//! These drive the built `sec-mcp` binary over stdio (JSON-RPC, exactly as an
//! MCP client would) against the **real** SEC EDGAR APIs. They are **skipped by
//! default** — `cargo test` stays offline — and run only when
//! `SEC_MCP_LIVE_EMAIL` is set, which both opts in and supplies the contact
//! email EDGAR's fair-access policy requires:
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

use serde_json::{json, Value};

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
        let sandbox =
            std::env::temp_dir().join(format!("sec-mcp-live-{}", std::process::id()));
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
        Server { child, stdin, stdout, sandbox, next_id: 1 }
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
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return v;
            }
        }
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
    let init = server.request("initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert_eq!(init["result"]["serverInfo"]["name"], "sec-mcp", "init: {init}");
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
    assert_eq!(rows[0]["form"].as_str(), Some("10-K"), "first row form: {filings}");

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
