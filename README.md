# sec-mcp

A [Model Context Protocol](https://modelcontextprotocol.io/) server that gives Claude Desktop access to SEC EDGAR data — company filings, financial statements, and company info. No API key needed.

## Why an MCP for this?

EDGAR is a set of public REST endpoints with no API key, and the documentation at [sec.gov/search-filings/edgar-application-programming-interfaces](https://www.sec.gov/search-filings/edgar-application-programming-interfaces) is reasonable. So strictly speaking, an MCP isn't needed. What this wrapper buys you:

1. **Persistent User-Agent identity.** SEC's fair-access policy requires a contact email in the User-Agent header; without one, requests get throttled or rejected. You don't want to re-paste your email into every prompt. The MCP stores it in a config file and stamps every request.

2. **Ticker → CIK resolution with caching.** Every EDGAR endpoint is keyed by a zero-padded 10-digit CIK, not by ticker. Without a tool, each query would re-fetch the ~1 MB `company_tickers.json` and parse it from scratch. The MCP caches it for an hour.

3. **URL-construction quirks the docs are quiet about.** The `CY{period}` prefix on the XBRL frames endpoint, the trailing `I` for instant concepts, the zero-padded CIK, the dash-stripped accession number in archive URLs. Easy to get wrong on the fly; encoded once in the tool.

4. **Response shaping.** `companyfacts` responses are tens of MB and `filings.recent` is laid out as column-major parallel arrays. Both are usable, but a language model would burn a lot of context reformatting them. The MCP returns trimmed, row-shaped JSON.

5. **Discovery.** With the MCP installed, the tool list itself tells Claude that it can query SEC filings, plus the common concept names. Without it, the model has to remember the endpoints exist.

If you only pull one or two filings a year, you don't need this — just give Claude the URL. At any higher volume, the wrapper pays for itself by turning "explain the URL grammar in every conversation" into "ask a natural question."

## Install

### Option 1: Download a prebuilt binary (no Rust required)

1. Download the latest binary for your Mac from the [Releases](https://codeberg.org/brechanbech/sec-mcp/releases) page:
   - `sec-mcp-aarch64-apple-darwin` — Apple Silicon (M1/M2/M3/M4)
   - `sec-mcp-x86_64-apple-darwin` — Intel

2. Make it executable and move it into your PATH. Replace `<arch>` with either `aarch64` or `x86_64` to match the binary you downloaded:
```zsh
chmod +x sec-mcp-<arch>-apple-darwin
sudo mv sec-mcp-<arch>-apple-darwin /usr/local/bin/sec-mcp
```

### Option 2: Install with Cargo

If you have [Rust](https://rustup.rs) installed (1.85 or newer):
```zsh
cargo install sec-mcp
```

Building from source compiles the rustls crypto backend (`ring`), which has a
small amount of C, so you need a **C compiler** on the build host — but no
`cmake` or assembler. macOS (Xcode Command Line Tools) and most desktop Linux
distros already have one; a minimal Linux image may not — install it first,
e.g. `apt install build-essential` on Debian/Ubuntu.

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
- *"Show me Amazon's most recent 10-K filing"*
- *"What tickers are listed on NYSE with 'energy' in the name?"*
- *"What financial metrics does Apple report to the SEC?"*
- *"Which companies had the highest revenue in 2024?"*

## Available tools

| Tool | Description |
|------|-------------|
| `sec_configure` | First-run setup — saves your contact email |
| `sec_lookup_cik` | Resolve a ticker symbol to its SEC CIK number |
| `sec_company_info` | SIC code, industry, state of incorporation, fiscal year end, addresses |
| `sec_recent_filings` | Recent filings (10-K, 10-Q, 8-K, etc.) with direct URLs — pages into older history automatically when a form-type filter needs it |
| `sec_financial_concept` | Historical financial data from XBRL (revenue, net income, EPS, assets…) |
| `sec_list_tickers` | Search/list all SEC-registered tickers with exchange info |
| `sec_company_facts` | Discover all XBRL concepts (metrics) a company reports |
| `sec_xbrl_frames` | Cross-company comparison of a financial metric for a given period |

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

## License

MIT — see [LICENSE.md](LICENSE.md) for details.

## MCP registry

Ownership-verification token for the [MCP registry](https://registry.modelcontextprotocol.io)
(read from this crate's rendered README on crates.io):

> Registry ownership token: `mcp-name: io.github.brechanbech/sec-mcp`
