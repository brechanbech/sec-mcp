# sec-mcp roadmap

Candidate EDGAR capabilities that are **not** in sec-mcp today, recorded so the
reasoning survives rather than being re-derived each time. Nothing here is
committed work; the ordering reflects value-per-effort, not priority.

Current scope is the four documented `data.sec.gov` APIs — submissions, XBRL
company-concept, company-facts, and frames — surfaced as eight tools. Verified
against SEC's [EDGAR APIs page](https://www.sec.gov/search-filings/edgar-application-programming-interfaces)
on 29 July 2026: all four are current, none deprecated, no auth required, 10
req/sec, `User-Agent` mandatory. EDGAR Next (mandatory filer enrolment since
24 March 2025) governs *submitting* filings and does not affect data consumers.

## 1. Full-text search

`https://efts.sec.gov/LATEST/search-index?q=…&forms=…&dateRange=…`

Searches the **body text** of every filing since 2001, including exhibits.
Verified working July 2026. Returns an Elasticsearch-shaped response whose
`_source` carries `ciks`, `display_names` (with ticker), `form`, `file_date`,
`adsh` (accession), `sics`, `biz_locations`, `inc_states`, `period_ending`, and
8-K `items`; the hit `_id` is `accession:document`, so document URLs are
constructible.

This is the single biggest change in what can be *asked*. Every current tool
requires you to already know the company; full-text search finds companies **by
what they said** — "who discussed a climate transition plan in a 2025 10-K".

**Effort: low.** One endpoint, regular response shape, maps to a tool about the
size of the existing ones.

**Risk: it is public but undocumented** — absent from SEC's official APIs page,
so no stability guarantee. Worth keeping in proportion: sec-mcp already depends
on undocumented EDGAR behaviour and the README sells that as a feature (the `CY`
prefix, the trailing `I` for instant concepts, `-per-` compound units,
column-major `filings.recent`, older-history paging). The mitigation already
exists too — `tests/live_smoke.rs` is explicitly there to catch EDGAR drifting
under us, so an assertion belongs there from day one.

## 2. Filing document retrieval

`https://www.sec.gov/Archives/edgar/data/{cik}/{accession-nodash}/index.json`
enumerates every document in a filing plus the XBRL zip (verified working).

`sec_recent_filings` currently returns URLs the model cannot read unless the
client happens to have its own fetch tool. Closing that loop — from "here is a
10-K" to "here is what it says" — is the natural completion of the filing tools,
and pairs naturally with full-text search: find the filing, then read it.

**Effort: high, and the cost is design not maintenance.** The endpoint is
trivial; the payload is not. Filings are multi-megabyte HTML/iXBRL, so this
needs a policy for *which* document (primary? which exhibit?), *how much* (a
whole 10-K will not fit a context window), and *what transformation* (strip
markup? extract sections? paginate?). That is the same problem README point 4
already solves for JSON responses, but materially harder for prose. Do not start
this without deciding the shaping strategy first.

## 3. Insider transactions (Forms 3/4/5)

Visible in `submissions` today but unparsed. Form 4 is well-specified structured
XML: who bought or sold, when, how much, at what price. "Has anyone at this
company been selling?" is an ordinary question sec-mcp cannot currently answer.

**Effort: moderate.** Parse a defined XML schema into slim JSON — the same shape
of work as the existing XBRL response shaping, against a more stable input.

## 4. 13F institutional holdings

Quarterly holdings disclosures from institutional managers. Same character as
item 3: structured, defined, parse-and-slim. Answers "who holds this stock".

**Effort: moderate.**

## Explicitly rejected

**Bulk ZIP archives.** SEC offers `companyfacts.zip` and `submissions.zip`
(`/Archives/edgar/daily-index/…`, rebuilt nightly ~03:00 ET) and calls them "the
most efficient means to fetch large amounts of API data". True for batch
pipelines, wrong for this: they are multi-gigabyte and an MCP server answers
interactive questions. The per-request APIs plus the existing one-hour ticker
cache are the right fit.

## Known limitations

- **Amendments are excluded from period filters.** `annual`/`quarterly` match
  base forms only, so a `10-K/A` restatement is not returned. Deliberate — the
  amended period is already present via the original filing, and admitting both
  would emit two facts for one period. Revisit if restatements matter.
- **`sec_list_tickers` caps at 50 rows** and `sec_financial_concept` returns the
  last 20 facts. Fine interactively; a caller wanting completeness has no way to
  page past it.
