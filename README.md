# sec-mcp

A [Model Context Protocol](https://modelcontextprotocol.io/) server that gives Claude Desktop access to SEC EDGAR data — company filings, financial statements, and company info. No API key needed.

## Install

### Option 1: Download a prebuilt binary (no Rust required)

1. Download the latest binary for your Mac from the [Releases](https://codeberg.org/brechanbech/sec-mcp/releases) page:
   - `sec-mcp-aarch64-apple-darwin` — Apple Silicon (M1/M2/M3/M4)
   - `sec-mcp-x86_64-apple-darwin` — Intel

2. Make it executable and move it into your PATH:
```zsh
   chmod +x sec-mcp-aarch64-apple-darwin
   sudo mv sec-mcp-aarch64-apple-darwin /usr/local/bin/sec-mcp
```

### Option 2: Install with Cargo

If you have [Rust](https://rustup.rs) installed:
```zsh
cargo install sec-mcp
```

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

## Available tools

| Tool | Description |
|------|-------------|
| `sec_configure` | First-run setup — saves your contact email |
| `sec_lookup_cik` | Resolve a ticker symbol to its SEC CIK number |
| `sec_company_info` | SIC code, industry, state of incorporation, fiscal year end, addresses |
| `sec_recent_filings` | Recent filings (10-K, 10-Q, 8-K, etc.) with direct URLs |
| `sec_financial_concept` | Historical financial data from XBRL (revenue, net income, EPS, assets…) |

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

MIT OR Apache-2.0
