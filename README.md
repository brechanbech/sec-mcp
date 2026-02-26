# sec-mcp

A [Model Context Protocol](https://modelcontextprotocol.io/) server written in Rust that exposes SEC EDGAR data as tools for Claude Desktop.

No API key required — the SEC EDGAR APIs are free and public.

## Tools

| Tool | Description |
|------|-------------|
| `sec_configure` | **First-run setup** — saves your contact email to `~/.config/sec-mcp/config.toml` |
| `sec_lookup_cik` | Resolve a ticker symbol to its SEC CIK number |
| `sec_company_info` | SIC code, industry, state of incorporation, fiscal year end, addresses |
| `sec_recent_filings` | Recent filings (10-K, 10-Q, 8-K, etc.) with direct URLs |
| `sec_financial_concept` | Historical financial data from XBRL (revenue, net income, EPS, assets…) |

## First-run behaviour

The SEC [fair-access policy](https://www.sec.gov/os/accessing-edgar-data) requires automated clients to include a contact address in the HTTP `User-Agent` header. On first use Claude will:

1. Notice that no email is configured (the `sec_configure` tool description signals this).
2. Explain why the email is needed and ask for your permission.
3. Call `sec_configure` with the email you provide.
4. Save it to `~/.config/sec-mcp/config.toml` — it persists across restarts.

From then on Claude goes straight to fetching data. You can update the email at any time by asking Claude to reconfigure.

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

## Install
```zsh
cargo install sec-mcp
```

Or download a prebuilt macOS binary from the [Releases](https://codeberg.org/brechanbech/sec-mcp/releases) page.

## Configure Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "sec-edgar": {
      "command": "/Users/YOUR_USERNAME/.cargo/bin/sec-mcp",
      "args": []
    }
  }
}
```

Restart Claude Desktop. On your first SEC-related request Claude will ask for your email before proceeding.

## Logging

Logs go to stderr (not stdout, which is reserved for MCP JSON-RPC). To enable debug output:
```zsh
RUST_LOG=sec_mcp=debug sec-mcp
```
