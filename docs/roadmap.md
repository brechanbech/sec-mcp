# sec-mcp roadmap

Candidate EDGAR capabilities that are **not** in sec-mcp today, recorded so the
reasoning survives rather than being re-derived each time. Nothing here is
committed work; the ordering reflects value-per-effort, not priority.

Current scope is the four documented `data.sec.gov` APIs — submissions, XBRL
company-concept, company-facts, and frames — plus the undocumented full-text
search on `efts.sec.gov`, and ownership XML and filing documents from the Archives,
surfaced as fourteen tools. Verified
against SEC's [EDGAR APIs page](https://www.sec.gov/search-filings/edgar-application-programming-interfaces)
on 29 July 2026: all four are current, none deprecated, no auth required, 10
req/sec, `User-Agent` mandatory. EDGAR Next (mandatory filer enrolment since
24 March 2025) governs *submitting* filings and does not affect data consumers.

**Re-verified 18 September 2026** — unchanged. The APIs page still documents the
same four endpoints and still carries its June 2024 update / April 2025 review
dates: no new consumer APIs, no deprecations, no change to the rate limit or the
`User-Agent` requirement. 2026's EDGAR releases are all filer-side and do not
touch the consumption path: [26.0.1](https://www.sec.gov/newsroom/whats-new/edgar-release-2601)
documents a 404 for the Enrollment API removed in 25.4,
[26.1](https://www.sec.gov/newsroom/whats-new/edgar-release-261) adds degraded
statuses to the Operational Status API, and
[26.3](https://www.sec.gov/submit-filings/edgar-news-announcements/edgar-release-263)
(14 September 2026) adds Form 1 and ANE Exception Notice interfaces plus XBRL
taxonomy versions. Same category as EDGAR Next.

**Re-verified 23 October 2026**: still unchanged. Same four endpoints, same
June 2024 / April 2025 dates on the APIs page. The only announcement since 26.3
is the [7 October preview of Release 26.4](https://www.sec.gov/submit-filings/edgar-news-announcements/preview-planned-technical-specifications-december-2026-edgar-release-264),
scheduled for **7 December 2026**. It adds one filer-side XML form (the SBS
Dispute Notice under Exchange Act Rule 15fi-3(c)) and says nothing about
`data.sec.gov`, full-text search, the Archives, ownership schemas, rate limits,
`User-Agent` or usage policy. The live smoke test was run end to end the same
day. It caught two things that were ours, not EDGAR drift: the ignored Form 3
`noSecuritiesOwned` flag, and full-text search's form filter admitting
amendments (see item 1).

## Watching: the EDGAR Beta API ecosystem preview

Not a capability to build — a change that could arrive under us, which is why it
is recorded here rather than left to be rediscovered.

SEC says ["substantial updates to the SEC's API ecosystem"](https://www.sec.gov/newsroom/whats-new/edgar-beta-will-remain-open)
will be previewed in EDGAR Beta, "including but not limited to, changes to API
technical specifications, the EDGAR API Development Toolkit, and **API usage
requirements or policies**."

That last clause is the one that matters here. Usage requirements are where a
rate limit, a `User-Agent` rule, or an authentication step would live, and all
three are load-bearing for this crate — the 10 req/sec limit shapes
`sec_insider_transactions`' sequential fetches, and the mandatory `User-Agent`
is why `sec_configure` exists at all.

**The scope is genuinely unstated.** The announcement does not say whether this
covers only the filer submission APIs or the `data.sec.gov` consumer APIs too,
and both plausibly sit under "API ecosystem". Do not assume either way; the
answer is worth an email to FilerTechUnit@sec.gov if it starts to matter.

**There is warning if anyone looks.** Major releases reach EDGAR Beta about two
weeks before live EDGAR. 26.4 is scheduled for 7 December 2026, and its preview
gives no Beta date, so **revisit in late November 2026**. Its published scope is
filer-side only, but the Beta preview is where a usage-policy change would
first appear (checked 23 October 2026).

## 1. Full-text search — **done**

Shipped as `sec_full_text_search` (18 September 2026). The notes below are kept
as the record of what the implementation had to handle.

The live parameters, confirmed on 18 September 2026, are the UI's own: `q`
(quote the string for a phrase match), `forms`, `ciks` (zero-padded 10-digit),
`startdt`/`enddt` as `YYYY-MM-DD`, and `from` for paging. Page size is **100**
and `hits.total.value` **saturates at 10000** with `relation: "gte"` — the tool
flags that as `total_is_lower_bound` rather than reporting a saturated count as
exact. A hit `_id` is `accession:document`, so the Archives URL is constructible
— note it takes the CIK *without* zero-padding and the accession *without*
dashes, the opposite of every `data.sec.gov` path. Two generated URLs were
fetched and returned 200.

**`forms` matches the root form, amendments included** (found 23 October
2026). EFTS echoes the Elasticsearch query it ran, and the filter is
`terms: { root_forms: [...] }`, so `forms=10-K` also returns every `10-K/A`.
The `form_filter` aggregation is keyed on `root_forms` too, so EDGAR offers no
count of amendments to subtract. The tool now drops unrequested amendments
client-side, consistent with the period filters, and is honest about the cost:
`total` is flagged as an upper bound, and paging resumes from `next_offset`,
not `offset + returned`. The September smoke test passed only because a base
10-K happened to rank first.

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

**Done** (26 October 2026): `sec_filing_text`, designed in [`filing-text-design.md`](filing-text-design.md).

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

**The benchmark below makes this the largest gap (recorded 26 October 2026).**
Of the 50 public Finance Agent Benchmark questions, a rough classification puts
only about 10–12 within reach of the XBRL tools: revenue CAGR, inventory
turnover, effective tax rate, lease maturities, free cash flow, payout ratios.
The rest need prose:

- **Guidance lives in 8-K exhibits.** The seven Beat or Miss questions, and
  several modelling ones, compare a figure guided in one quarter's earnings
  release (exhibit `EX-99.1`) with the actual in a later one. Reaching an
  exhibit means walking `index.json`, not just fetching the primary document.
- **Qualitative answers live in 10-K text**: the CFO, risk factors, a metric's
  definition, a distribution centre's start date.
- **Operating KPIs are not us-gaap facts.** Gross bookings, nights per booking,
  ARPU and same-store sales sit in MD&A or the earnings release, not in XBRL
  company facts.
- **Some answers are in the proxy** (`DEF 14A`): director compensation.

A shaping strategy that would serve those questions: take an accession (and
optionally a document name), list the filing's documents from `index.json`,
and return one document as plain text, paged by `offset`/`limit` like the other
tools. For 10-K and 10-Q, also return a section index (Item 1A, Item 7, …) so a
caller can jump rather than page. HTML-to-text needs a new dependency, and it
should be pure Rust, consistent with the crate's no-build-time-C stance beyond
`ring`.

**Can the client already read the URLs? Tested in Claude Desktop, 26 October
2026.** Access is not the problem; size is. Its web fetch read only the first
~120k of the FY2024 Airbnb 10-K's ~430k characters, stopping before the MD&A,
so a benchmark question answered there (Asia Pacific nights per booking, 3.3)
failed through fetch. It succeeded only by falling back to the app's built-in
browser, after the user approved sec.gov there. The Q1 2025 earnings exhibit
(`EX-99.1`, 56 KB of HTML) was read in full, and the chain from question to
exhibit worked, also through the browser. Most MCP clients have no such
browser. So the case for this item is that deep documents become reachable in
any client, without a browser or an approval prompt. That is narrower than
"the only way to read a filing", and it is what paging and a section index buy.

That exhibit is also a shareholder letter with 27 full-page JPEGs beside its
text. The guidance was in the text, but a text tool cannot see what a filer
publishes only as images, and should say so rather than imply completeness.

Precedent: edgar-audit-mcp (Python, a 12-tool server) ships
`sec_edgar_read_filing_text`, and it is what lets its tool-only arm answer the
qualitative and Beat or Miss questions. Here it would be one new tool, fed by
the accessions `sec_recent_filings` and `sec_full_text_search` already return.

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

## 4. 13F institutional holdings — **done** (the tractable half)

**Not low-hanging — and not for parsing reasons.** The information table is
regular XML (`nameOfIssuer`, `titleOfClass`, `cusip`, `value`, `sshPrnamt`,
voting authority; ~45 KB / 90 positions for Berkshire's 2026-05-15 filing). The
obstacles are elsewhere, and two of the three sit outside the code:

1. **The useful direction is the expensive one.** A 13F is filed *by the
   manager*, listing what they hold. "What does manager X hold?" is a simple
   lookup by CIK. "Who holds AAPL?" — the question people actually want — means
   scanning every manager's filing, thousands per quarter. There is no
   holder-by-security index.
2. **sec-mcp could not identify most managers.** It was ticker-driven, and most
   institutional managers have no ticker. Item 5 removed this obstacle on
   26 October 2026: `sec_lookup_cik` with `name: "Bridgewater Associates"`
   now returns its CIK.
3. **Holdings are keyed by CUSIP, and CUSIP is licensed.** The table carries no
   ticker. CUSIP Global Services is operated by FactSet on behalf of the ABA, so
   there is no free SEC-published CUSIP→ticker crosswalk. Even "does Berkshire
   hold Apple?" degrades to fuzzy-matching `nameOfIssuer` text.

**If revisited, scope it honestly to the tractable half**: "what does manager X
hold", taking a CIK, reporting issuer names as filed. That is a real capability,
but a much smaller promise than "who holds this stock". Item 5 has removed obstacle 2.

**Done** (26 October 2026) as `sec_institutional_holdings`, the fourteenth tool,
scoped as above.

- **Finding the table.** The information table is a separate XML file named by
  the filing agent (`56757.xml`, `form13fInfoTable.xml`). The filing header
  types it `INFORMATION TABLE`, so `sec_filing_text`'s header reader finds it.
- **Aggregating rows.** The table has a row per position per sub-manager and
  discretion: Apple is 12 of Berkshire's 89 rows. Rows are summed by CUSIP,
  class and put/call, and the positions add up exactly to the cover page's
  `tableValueTotal`. That holds for Berkshire (.25B) and for BlackRock
  (49,968 rows, 5,696 positions, .73T).
- **Units.** Values are dollars for filings made from 3 January 2023, and
  thousands before. Older reports are converted.
- **Size.** BlackRock's table is 23 MB. It is capped at 64 MB and parsed on a
  blocking thread: 1.8 s of CPU and about 200 MB resident in a debug build.
- **Amendments.** Those for the period are listed, not merged. A 13F-HR/A
  either restates the report or adds holdings, and only its cover page says
  which.
- **What it says about itself.** The description points "who holds a stock"
  to `sec_beneficial_ownership`, and says what 13F leaves out: shorts, cash,
  foreign shares and confidential positions.

## Beyond the documented API (recorded 24 October 2026)

With item 1 and item 3 shipped, sec-mcp uses all four documented `data.sec.gov`
APIs. Everything below is in the Archives or on undocumented endpoints, so each
needs a `tests/live_smoke.rs` assertion from day one, as full-text search did.

**Tool count is now a cost, not a score.** Ten tools rated 5/5 on Glama's
tool-count dimension ("a well-scoped set"). Every tool added costs context in
every session and makes the right tool harder to pick, so before adding one,
ask whether it extends an existing tool instead. The notes below say where it
could.

## 5. Company name → CIK

**Done** (26 October 2026): `sec_lookup_cik` takes a `name` beside `ticker`,
backed by the EDGAR company autocomplete below. `name: "Bridgewater
Associates"` returns CIK 1350694 first. Abbreviations do not expand ("US Steel"
finds Kobe Steel USA, not CIK 1163302), and people are filed surname first;
the tool description says both.

Every tool started from a ticker, so filers without one were unreachable: most
13F managers, private issuers filing Form D, funds, individual insiders.
`sec_lookup_cik("Bridgewater Associates")` failed. Two sources, both
verified 24 October 2026:

- **`https://www.sec.gov/Archives/edgar/cik-lookup-data.txt`**: every EDGAR
  entity as `NAME:CIK:` lines. About 40 MB (11.6 MB gzipped), 1.06 million
  lines, rebuilt daily. It is complete and needs no undocumented endpoint, but
  it is too large to fetch per request, so it would need a disk cache, and
  names are as filed, with no ranking and frequent near-duplicates
  (`… HOLDINGS, INC.` and `… HOLDINGS, LLC` on one CIK).
- **`https://efts.sec.gov/LATEST/search-index?keysTyped=<name>`**: the
  company autocomplete behind EDGAR's search box. It is ranked, returns
  tickers where they exist, and costs one small request. `bridgewater
  associates` returns "Bridgewater Associates, LP" (CIK 1350694) first. It is
  undocumented, like full-text search on the same host.

The autocomplete is the better fit for interactive use. **It could extend
`sec_lookup_cik`** (a `name` parameter beside `ticker`) rather than add a tool.
**Effort: low.** It unblocks item 4 and the Form D half of item 7.

## 6. Latest filings — **done**

**Done** (26 October 2026) without a new tool, against the note below. With
thirteen tools the count mattered more than when this was written:

- **`sec_recent_filings` with no `ticker`** reads the feed: real time, about
  two business days deep (some 520 8-Ks), 100 entries a page. A filing with
  several parties is one entry per party (Reporting/Issuer, Filed by/Subject),
  so entries are grouped by accession. `type` matches amendments too. The body
  declares ISO-8859-1 and the response names no charset, so it is decoded as
  Latin-1 when it is not valid UTF-8.
- **`sec_full_text_search` without a `query`** lists filings by form and date
  for any range back to 2001. With no query EDGAR returns one hit per filing
  (its primary document), newest first. That covers the past days the feed
  does not, so the daily indexes were not needed.

The original note:

Every tool starts from a company. Nothing answers a question that starts from
*time*: "which 8-Ks were filed today", "any new 13D this week".
`https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-K&count=…&output=atom`
returns an Atom feed of current filings, filterable by form (verified
24 October 2026). The daily indexes under `/Archives/edgar/daily-index/` cover
past days. Full-text search with a date range overlaps but needs a query
string. **Effort: low to moderate**: the Atom feed needs an XML parse, which
`roxmltree` already provides. This is a genuinely new kind of question, so a new
tool is justified here.

## 7. More structured ownership and offering forms — **done**

The same pattern as `sec_insider_transactions`: find the filing through
submissions or full-text search, then read its `primary_doc.xml`. All three
were confirmed as XML against live October 2026 filings.

- **Form 144**, notice of a proposed sale by an insider. **Done** (26 October
  2026) as `form_type: "144"` on `sec_insider_transactions`, with no new tool.
  Electronic filing has been mandatory since 13 April 2023 (namespace
  `http://www.sec.gov/edgar/ownership`, but its own schema, not
  `ownershipDocument`). It pairs with Forms 3/4/5: a 144 is filed when the order
  is placed, and the Form 4 reporting the executed sale follows. The default
  form filter stays 3, 4 and 5, so existing callers see no change. Notices are
  parsed as filed: one of Tim Cook's (October 2026) values 218,078 shares at
  $3.3M, and the tool reports that rather than correcting it.
- **Schedules 13D and 13G**, holders above 5%. **Done** (26 October 2026) as
  `sec_beneficial_ownership`, a new tool rather than an extension of
  `sec_insider_transactions`: the filers, schemas and questions all differ.
  Structured XML has been mandatory since 18 December 2024 (schema `X0202`,
  namespaces `http://www.sec.gov/edgar/schedule13D` and
  `http://www.sec.gov/edgar/schedule13g`, forms `SCHEDULE 13D` and
  `SCHEDULE 13G`). Older filings are `SC 13D` and `SC 13G`, in HTML or text.
  They are listed with `structured: false` for `sec_filing_text` to read, not
  scraped. Schedules are indexed under the subject company and each filer, so
  the tool answers both "who holds X" and, given a holder's CIK from item 5,
  "what does Y hold above 5%".
- **Form D**, exempt offerings and private placements (schema `X0708`, no
  namespace). **Done** (26 October 2026) as `sec_private_offerings`, a new
  tool: the filer is the issuer itself, often a private company or fund, and
  the question is money raised, which no other tool answers. Most issuers have
  no ticker, so it is reached through item 5. Amendments are kept, since a
  fund files a `D/A` every year with the running total sold. Every Form D on
  EDGAR has been XML since 2009, so there is no older format to list.

**Effort: moderate per form**, but the parser and the `get_text` path exist
already. 13D/13G was the most valuable of the three: it is the only free,
structured answer to "who holds more than 5% of this company", the question
item 4 cannot answer. All three are done; with them sec-mcp has thirteen
tools.

## Measuring: the Finance Agent Benchmark (recorded 26 October 2026)

Not a capability: a yardstick for the ones above, and the evidence that ranks
item 2 first.

[Vals AI Finance Agent Benchmark](https://huggingface.co/datasets/vals-ai/finance_agent_benchmark),
CC BY 4.0. It has 537 expert-written questions over SEC filings, of which **50
are public**. Each public row carries the question, the expected answer, its
category (nine, from Quantitative Retrieval to Financial Modeling), an expert
time estimate, and a **rubric**: `correctness` criteria the answer must contain
and a `contradiction` check. The questions were written against filings as of
the dataset's publication, 16 May 2025.

**What others have reported, and how far it carries.** The authors' paper
([arXiv 2508.00828](https://arxiv.org/abs/2508.00828)) reports o3 best at 46.8%
on the full 537, with web search and an EDGAR tool. edgar-audit-mcp reports 90%
with its tools against 26% with none, on the public 50, graded by its own LLM
judge. Its write-up (`evaluation/benchmark.md` in that repo) is candid that this
is not comparable to the paper. The figure that holds up there is the paired
one: 32 questions fixed by the tools, none broken. That proves tools help;
it does not place a server on a leaderboard.

**Plan: run it twice, before and after item 2.** Run it before as a baseline,
then after, so the measurement isolates what filing text adds:

1. **Answering arm**: headless `claude -p`, allowed only sec-mcp's tools (no
   web search, no fetch), a budget of about eight tool calls per question, and
   "not found with these tools" an accepted answer.
2. **Point in time.** Instruct a cutoff of **2025-05-16**: use only filings
   filed on or before it. edgar-audit-mcp's first run lost five answers to
   "most recent" having drifted fifteen months past the questions, and fixed it
   with an `as_of` parameter. Instruction first; a parameter only if the run
   shows the instruction leaking, and then on filing date, not period end,
   since a restatement shares the original's period. Audit afterwards: every
   accession the arm cites must predate the cutoff.
3. **Grading against the dataset's own rubric**, not a free-form judge: each
   `correctness` criterion met or not, plus the `contradiction` check. That is
   closer to the paper's method and checkable per question.
4. **Report** raw and class-balanced accuracy (the paper's metric), with the
   Wilson interval: on n = 50 it is wide, 79–96% around a 90% score. Publish the
   raw answers and grades beside the number.

**Cost**: 50 questions × up to eight calls each, well inside EDGAR's rate limit
when run sequentially, plus one grading pass. **Attribution**: CC BY requires
crediting Vals AI wherever results are published. **Expectation for the
baseline**: low, and mostly a measure of item 2's absence. That is the point
of running it first.

**Baseline, 26 October 2026** ([`evaluation/runs/2026-10-26-baseline/`](../evaluation/runs/2026-10-26-baseline/notes.md)):
11 of 50 correct (22%), class-balanced 21.6%, and no citation after the cutoff.
Most failures are item 2 as expected. It also found two bugs cheaper than item
2, which should come first:

- **Accept a CIK wherever a ticker is taken.** Four questions failed because
  the company has no current ticker (US Steel, Spirit Airlines, Warner Bros.
  Discovery). Any point-in-time question meets delisted and renamed companies.
- **Page or filter `sec_company_facts`, and tighten `sec_insider_transactions`.**
  Six responses of 94k–181k characters exceeded Claude Code's tool-output limit
  and were lost to the agent.

Both fixed in 0.7.0. **After item 2, 26 October 2026**
([`evaluation/runs/2026-10-26-filing-text/`](../evaluation/runs/2026-10-26-filing-text/notes.md)):
31 of 50 correct (62%), class-balanced 52.9%, and none unanswered. Paired with
the baseline, 20 questions were fixed and none broken. The remaining wrong
answers are choices of method or period, not tool failures.

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
  would emit two facts for one period. **Opt-in added** (26 October 2026):
  `include_amendments: true` admits them, and `amended_facts` counts them, as
  `sec_full_text_search` already allowed. Apple's fiscal 2009 revenue reads
  $36.537B on the 10-K and $42.905B on the January 2010 10-K/A.
- **A filtered full-text page can come back short.** Dropping amendments
  happens after EDGAR's paging, so a page may return fewer than `limit` hits
  rather than spending extra requests to top it up. Callers page with
  `next_offset`.
- ~~**`sec_list_tickers` caps at 50 rows** and `sec_financial_concept` returns
  the last 20 facts.~~ **Done** (23 October 2026): both page with `limit`,
  `offset` and `next_offset`. `sec_financial_concept` pages back from the
  latest period end, which is EDGAR's sort key; `filed` is not monotonic.
