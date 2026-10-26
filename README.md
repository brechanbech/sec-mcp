# sec-mcp

[![audit](https://codeberg.org/brechanbech/sec-mcp/actions/workflows/audit.yml/badge.svg)](https://codeberg.org/brechanbech/sec-mcp/actions)

A [Model Context Protocol](https://modelcontextprotocol.io/) server that gives Claude Desktop access to SEC EDGAR data — company filings, financial statements, and company info. No API key needed. It runs as a single native binary, with no Node, Python or Docker runtime behind it.

## Why an MCP for this?

EDGAR is a set of public REST endpoints with no API key, and the documentation at [sec.gov/search-filings/edgar-application-programming-interfaces](https://www.sec.gov/search-filings/edgar-application-programming-interfaces) is reasonable. So strictly speaking, an MCP isn't needed. What this wrapper buys you:

1. **Persistent User-Agent identity.** SEC's fair-access policy requires a contact email in the User-Agent header; without one, requests get throttled or rejected. You don't want to re-paste your email into every prompt. The MCP stores it in a config file and stamps every request.

2. **Ticker → CIK resolution with caching.** Every EDGAR endpoint is keyed by a zero-padded 10-digit CIK, not by ticker. Without a tool, each query would re-fetch the ~1 MB `company_tickers.json` and parse it from scratch. The MCP caches it for an hour.

3. **URL-construction quirks the docs are quiet about.** The `CY{period}` prefix on the XBRL frames endpoint, the trailing `I` for instant concepts, the zero-padded CIK, the dash-stripped accession number in archive URLs. Easy to get wrong on the fly; encoded once in the tool.

4. **Response shaping.** `companyfacts` responses are tens of MB and `filings.recent` is laid out as column-major parallel arrays. Both are usable, but a language model would burn a lot of context reformatting them. The MCP returns trimmed, row-shaped JSON.

5. **Discovery.** With the MCP installed, the tool list itself tells Claude that it can query SEC filings, plus the common concept names. Without it, the model has to remember the endpoints exist.

If you only pull one or two filings a year, you don't need this — just give Claude the URL. At any higher volume, the wrapper pays for itself by turning "explain the URL grammar in every conversation" into "ask a natural question."

## Install

Install with [Cargo](https://rustup.rs) (Rust 1.88 or newer):
```zsh
cargo install sec-mcp
```

This compiles the rustls crypto backend (`ring`), which has a small amount of C,
so you need a **C compiler** on the build host — but no `cmake` or assembler.
macOS (Xcode Command Line Tools) and most desktop Linux distros already have one;
a minimal Linux image may not — install it first, e.g.
`apt install build-essential` on Debian/Ubuntu.

## Configure Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "sec-edgar": {
      "command": "sec-mcp",
      "args": []
    }
  }
}
```

Restart Claude Desktop.

## First use

On your first SEC-related request Claude will explain that a contact email is required by the [SEC EDGAR fair-access policy](https://www.sec.gov/os/accessing-edgar-data) and ask your permission before proceeding. Your email is stored locally in `~/Library/Application Support/sec-mcp/config.toml` and is only used in the HTTP User-Agent header sent to the SEC — it is not shared anywhere else.

## Example prompts

- *"What are Apple's recent SEC filings?"*
- *"Show me Tesla's annual revenue for the last 5 years"*
- *"What was Microsoft's net income last quarter?"*
- *"What industry is NVDA in according to the SEC?"*
- *"What is Bridgewater Associates' CIK, and what has it filed recently?"*
- *"Who owns more than 5% of Sonos, and has any of them filed a 13D?"*
- *"Show me Amazon's most recent 10-K filing"*
- *"What tickers are listed on NYSE with 'energy' in the name?"*
- *"What financial metrics does Apple report to the SEC?"*
- *"What revenue guidance did Airbnb give in its Q1 2025 earnings release?"*
- *"Summarize the risk factors in Tesla's latest 10-K"*
- *"Which companies had the highest revenue in 2024?"*

## Available tools

| Tool | Description |
|------|-------------|
| `sec_configure` | First-run setup — saves your contact email |
| `sec_lookup_cik` | Resolve a ticker to its SEC CIK number, or search every EDGAR filer by name: fund managers, private issuers and individuals with no ticker included |
| `sec_company_info` | SIC code, industry, state of incorporation, fiscal year end, addresses |
| `sec_recent_filings` | Recent filings (10-K, 10-Q, 8-K, etc.) with direct URLs — pages into older history automatically when a form-type filter needs it |
| `sec_filing_text` | The text of a filing document or exhibit (10-K, 10-Q, 8-K, earnings release EX-99.1, proxy): its section index, any section by item, a search within it, or pages from any point. Given a filing, lists its documents by type |
| `sec_full_text_search` | Search the **body text** of filings since 2001, exhibits included — the only tool here that finds companies by what they *said*, rather than needing the company up front. Filter by form, ticker and date (amendments [excluded unless asked for](#amendments-10-ka-and-friends)); every hit carries a direct document URL |
| `sec_insider_transactions` | Forms 3, 4 and 5 — who bought or sold, when, how many shares and at what price, with transaction codes in plain language and footnotes resolved. Also Form 144, an insider's notice of a proposed sale, with any 10b5-1 plan date |
| `sec_beneficial_ownership` | Schedules 13D and 13G — who owns more than 5% of a company, with voting and dispositive power, percent of class and, for a 13D, the holder's stated purpose. Given a holder's CIK, what that holder owns above 5% |
| `sec_financial_concept` | Historical financial data from XBRL (revenue, net income, EPS, assets…) — latest 20 facts by default, paging back into older history |
| `sec_list_tickers` | Search/list all SEC-registered tickers with exchange info — 50 per page, with paging |
| `sec_company_facts` | Discover all XBRL concepts (metrics) a company reports |
| `sec_xbrl_frames` | Cross-company comparison of a financial metric for a given period |

Wherever a tool takes a ticker, it also takes a CIK (`1163302`, `0001163302` or
`CIK0001163302`). SEC's ticker list holds only current tickers, so a company that
was acquired, delisted or renamed is reachable only by CIK. `sec_lookup_cik` with
`name` finds it, and full-text search hits and frames rows carry one.

## Amendments (`10-K/A` and friends)

A form ending in `/A` amends one filed earlier. A `10-K/A` is often **not** a
complete annual report. Rule 12b-15 requires only the full text of each item
being changed, so the most common amendment is Part III alone (directors, pay,
ownership), filed when the proxy statement misses its 120-day window. Others add
or fix exhibits. The rest are restatements that replace the original's figures.
Apple's January 2010 `10-K/A`, for example, refiled fiscal 2009 under new
revenue-recognition rules.

sec-mcp therefore **excludes amendments unless you ask for them**:

- `sec_financial_concept`'s `annual`/`quarterly` period filters match base forms
  only. The amended period is already covered by the original filing, and
  admitting both would report two facts for one period.
- `sec_full_text_search` with a form filter drops amendments. EDGAR itself does
  not: it filters on a filing's *root* form, so `forms=10-K` also returns
  `10-K/A`. To get amendments, request the form directly (`10-K/A`), list both
  (`10-K,10-K/A`), or pass `include_amendments: true`. Any amendment returned is
  marked `amendment: true`.

Because that search filtering happens after EDGAR's, two parts of the response
behave differently from a plain search:

- **`total`** is still EDGAR's count, which includes the dropped amendments. It
  is flagged `total_is_upper_bound`, or `total_is_approximate` when it has also
  saturated at 10000. Each page reports `amendments_excluded`.
- **Paging** must use `next_offset`. A page can return fewer than `limit` hits
  because amendments were skipped, so `offset + returned` would revisit them.

## Common XBRL concepts for `sec_financial_concept`

| Concept | What it is |
|---------|-----------|
| `Revenues` | Total revenues |
| `NetIncomeLoss` | Net income / loss |
| `EarningsPerShareBasic` | Basic EPS |
| `EarningsPerShareDiluted` | Diluted EPS |
| `Assets` | Total assets |
| `Liabilities` | Total liabilities |
| `StockholdersEquity` | Shareholders' equity |
| `OperatingIncomeLoss` | Operating income / loss |
| `CashAndCashEquivalentsAtCarryingValue` | Cash and equivalents |
| `CommonStockSharesOutstanding` | Shares outstanding |

## Development

```zsh
cargo test          # offline; hits no network
```

There is also an opt-in live suite that drives the built server over stdio
against the **real** SEC EDGAR APIs — the thing offline tests can't see, like
EDGAR changing a response shape or a URL-construction quirk slipping through. It
is skipped unless `SEC_MCP_LIVE_EMAIL` is set, which both opts in and supplies
the contact email the [fair-access policy](https://www.sec.gov/os/accessing-edgar-data)
requires (the address is read from the environment, never committed):

```zsh
SEC_MCP_LIVE_EMAIL="you@example.com" cargo test --test live_smoke -- --nocapture
```

The suite runs sequentially through one server process, so it stays well within
EDGAR's rate limit.

## License

MIT — see [LICENSE.md](LICENSE.md) for details.

## MCP registry

Ownership-verification token for the [MCP registry](https://registry.modelcontextprotocol.io)
(read from this crate's rendered README on crates.io):

> Registry ownership token: `mcp-name: io.github.brechanbech/sec-mcp`

### Releasing

A version is immutable once published — crates.io yanks, it does not delete, and a number is
never reused — so the order matters:

1. `cargo test` and `cargo clippy --all-targets` clean.
2. Bump `version` in **`Cargo.toml`**.
3. Bump `version` in **`server.json`** — *both* fields, the top-level one and the one under
   `packages[]`. Nothing enforces this: no script, no CI, no test reads the file, and it is the
   step that goes missing. This manifest stayed correct, but the registry *entry* did not: 0.5.0,
   0.5.1 and 0.5.2 all shipped to crates.io while the registry still advertised 0.4.4, because
   step 6 was never run.
4. Commit and push; let CI go green before publishing rather than after.
5. `cargo publish --dry-run`, which compiles *from the packaged copy* and so catches a crate
   depending on a file it does not ship. Then `cargo publish`. Give both
   `--target-dir target/package-verify`. Without it, the verify build shares
   `target/debug` with ordinary builds under the same unit hash, so the
   binary's fingerprint ends up tracking the packaged `src/main.rs` rather
   than yours. Later edits then never rebuild `target/debug/sec-mcp`, and the
   offline stdio test in `tests/live_smoke.rs` silently runs the published
   code. If that has happened, `cargo clean -p sec-mcp` repairs it.
6. Publish to the MCP registry, **after** crates.io and not before — the registry verifies the
   namespace by reading `mcp-name` from this crate's *rendered README on crates.io*, and
   `server.json` names a cargo version that must already exist there:

   ```sh
   mcp-publisher validate      # checks server.json against the live registry
   mcp-publisher login github  # interactive, for the io.github.brechanbech/* namespace
   mcp-publisher publish
   ```

7. Create a release for the tag on **both** Codeberg and the GitHub mirror, with the
   version's CHANGELOG section as notes. A tag alone is not a release: Glama reads the
   GitHub mirror and reported "No stable releases found" while it carried every tag
   and no release.
