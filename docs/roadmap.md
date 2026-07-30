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

## 3. Insider transactions (Forms 3/4/5) — **done**

Shipped as `sec_insider_transactions` (30 July 2026). The XML-parser cost noted
below is now paid: `roxmltree` is in the tree and `EdgarClient::get_text` exists,
so any future work needing a non-JSON document starts from there. The notes below
are kept as the record of what the implementation had to handle.

Answers "has anyone at this company been buying or selling?" — an ordinary
question sec-mcp could not previously answer.

**Discovery already works.** Forms 3/4/5 are indexed under the *issuer's* CIK as
well as the insider's, so `sec_recent_filings` with `form_type: "4"` already
returns them (verified on AAPL, 30 July 2026). What is missing is reading the
document behind the URL.

**The payload is about as friendly as XML gets** — ~7.7 KB, flat and
self-describing:

```
issuer:         issuerCik / issuerName / issuerTradingSymbol
reportingOwner: rptOwnerCik / rptOwnerName / isOfficer / officerTitle
transactions:   transactionDate / transactionCode / transactionShares
                transactionPricePerShare / sharesOwnedFollowingTransaction
```

Note the filing URL in `sec_recent_filings` points at the XSL-rendered form
(`…/xslF345X06/form4.xml`); the raw XML is the same path with that segment
removed.

**Effort: low-to-moderate**, but it carries two firsts for this crate: the first
**XML parser** dependency (`quick-xml` or `roxmltree` — everything today is
`serde_json`) and the first **non-JSON fetch** (`RestClient::get_json` against
`data.sec.gov`; this pulls text from `www.sec.gov/Archives`). Those costs are
shared with item 4, so doing this first makes 13F cheaper later.

## 4. 13F institutional holdings

**Not low-hanging — and not for parsing reasons.** The information table is
regular XML (`nameOfIssuer`, `titleOfClass`, `cusip`, `value`, `sshPrnamt`,
voting authority; ~45 KB / 90 positions for Berkshire's 2026-05-15 filing). The
obstacles are elsewhere, and two of the three sit outside the code:

1. **The useful direction is the expensive one.** A 13F is filed *by the
   manager*, listing what they hold. "What does manager X hold?" is a simple
   lookup by CIK. "Who holds AAPL?" — the question people actually want — means
   scanning every manager's filing, thousands per quarter. There is no
   holder-by-security index.
2. **sec-mcp cannot identify most managers.** It is ticker-driven, and most
   institutional managers have no ticker: `sec_lookup_cik("Bridgewater
   Associates")` fails. Berkshire works only because it happens to be listed.
   This needs a name→CIK path the server does not have (SEC's
   `cik-lookup-data.txt` would serve, at the cost of another dataset to fetch
   and cache).
3. **Holdings are keyed by CUSIP, and CUSIP is licensed.** The table carries no
   ticker. CUSIP Global Services is operated by FactSet on behalf of the ABA, so
   there is no free SEC-published CUSIP→ticker crosswalk. Even "does Berkshire
   hold Apple?" degrades to fuzzy-matching `nameOfIssuer` text.

**If revisited, scope it honestly to the tractable half**: "what does manager X
hold", taking a CIK, reporting issuer names as filed. That is a real capability,
but a much smaller promise than "who holds this stock".

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
