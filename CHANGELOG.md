# Changelog

All notable changes to sec-mcp are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.4.2 are recorded only in the git tags (`v0.1.0`–`v0.4.1`).

## [Unreleased]

### Added
- **Latest filings across EDGAR.** `sec_recent_filings` with no `ticker` now
  answers questions that start from a time rather than a company: "which 8-Ks
  were filed today", "any new 13D this morning".
  - **Source.** It reads EDGAR's real-time latest-filings feed, filtered by
    `form_type`. The feed reaches back about two business days, newest first,
    and amendments match too.
  - **What each row gives.** One filing, with its acceptance time and index
    URL. It lists every party: a Form 4's reporting person and issuer, or a
    13D's filers and subject company, each with CIK and role. For an 8-K it
    also gives the items.
  - **Paging.** `next_offset` pages further back.
- **Filings by form and date, for any period.** `sec_full_text_search` no
  longer requires a `query`. With `forms` or a date range alone, it lists
  matching filings, one per filing and newest first, back to 2001: "every
  Schedule 13D filed last week". A call with neither a query nor a filter is
  refused rather than searching all of EDGAR.
  - **No new tool.** Both extend existing tools, so sec-mcp stays at thirteen.
- **`sec_financial_concept` takes `include_amendments`.** The `annual` and
  `quarterly` period filters still leave out amendments by default, since the
  original filing already reports the period. With `include_amendments: true`
  they admit `10-K/A`, `10-Q/A`, `20-F/A` and the like, and `amended_facts`
  counts them. This is for when the restatement is the point: Apple's fiscal
  2009 revenue reads $36.537B on the 10-K and $42.905B on the January 2010
  10-K/A.

## [0.9.0] - 2026-10-26

### Added
- **`sec_lookup_cik` finds filers by name.** A new `name` parameter, beside
  `ticker`, searches every entity EDGAR knows, not just those with a ticker.
  That includes fund managers such as Bridgewater, private issuers, and
  individual insiders. It returns up to 10 ranked matches, each with name, CIK
  and any tickers. Every tool takes a CIK in place of a ticker, so a name match
  reaches a filer's filings directly.
  - **Source.** It uses the autocomplete behind EDGAR's company search box, on
    `efts.sec.gov`. That endpoint is undocumented, like full-text search on the
    same host, so the live suite asserts on it.
  - **Matching.** Words match as prefixes in any order, but abbreviations do
    not expand: "US Steel" finds Kobe Steel USA, while "United States Steel"
    finds CIK 1163302. People are filed surname first ("Buffett Warren").
  - **Tool title.** It is now "Look Up CIK", no longer "Look Up CIK by Ticker".
- **`sec_beneficial_ownership` reads Schedules 13D and 13G**: who owns more
  than 5% of a company. It is the only free, structured source for that
  question; 13F holdings cannot answer it.
  - **What it returns.** Filings newest first. Each gives every reporting
    person's type in plain language (investment adviser, individual, …), sole
    and shared voting and dispositive power, aggregate shares and percent of
    class.
  - **13G and 13D.** A 13G names the rule it is filed under (passive or
    institutional holder) and flags a holder dropping to 5% or less. A 13D adds
    the holder's stated purpose (Item 4) and source of funds (Item 3), capped
    at 4,000 characters each.
  - **Either direction.** Given a company, it lists the holders that have
    reported on it. Given a holder's CIK, found with `sec_lookup_cik`'s `name`,
    it lists the companies that holder has reported on.
  - **Older filings.** Filings before 18 December 2024 are HTML or text. They
    are listed with `structured: false` and a URL for `sec_filing_text`, not
    scraped.
  - **Tool count.** This takes sec-mcp to twelve tools.
- **`sec_private_offerings` reads Form D**: money raised without a registered
  offering, by private companies, funds and public companies. Most filers have
  no ticker, so it is reached through `sec_lookup_cik`'s `name`.
  - **What it returns.** Filings newest first. Each gives the issuer (entity
    type, jurisdiction, state) and the industry group; for a fund, its type. It
    also gives the revenue or net-asset range, the exemption claimed in plain
    language (Rule 506(b), 506(c), Investment Company Act 3(c)(1) or 3(c)(7)),
    and the date of first sale. Then the amounts offered, sold and remaining (a
    fund's open-ended offering reads `Indefinite`) and the number of investors.
    Last come the commissions and finder's fees, the brokers paid, and the
    officers, directors and promoters named.
  - **Amendments.** `D/A` filings are included: a fund files one every year,
    with the running total sold.
  - **Tool count.** This takes sec-mcp to thirteen tools.
- **`sec_insider_transactions` reads Form 144** with `form_type: "144"`. A
  Form 144 is an insider's notice of a proposed sale, filed when the order is
  placed, before the Form 4 that reports the trade.
  - **What it returns.** Each notice gives the seller and their relationship
    to the company, the shares and market value to be sold, the broker,
    exchange and approximate date, and how the shares were acquired. It also
    gives any Rule 10b5-1 plan adoption date and the seller's sales in the past
    three months.
  - **Default unchanged.** Without `form_type` the tool still returns Forms 3,
    4 and 5.

## [0.8.0] - 2026-10-26

### Added
- **`sec_filing_text` reads the text of filings and exhibits.** Until now every
  tool returned structured XBRL or a URL. The prose was out of reach: guidance
  in an earnings release, risk factors, KPI definitions, deal terms. It
  accounted for most of the October 2026 Finance Agent Benchmark failures. MCP
  clients cannot be relied on to fill the gap: Claude Desktop's own fetch read
  only the first quarter of a 10-K.
  - **What it reads.** HTML documents, inline XBRL and agent-prepared exhibits
    alike, become plain text, each table row on one line with split `$`, `)`
    and `%` cells rejoined.
  - **Finding the right document.** Given a filing, it lists the documents by
    type, so an 8-K's EX-99.1 press release can be found.
  - **Navigating a long document.** The first page lists the section headings
    (`Part I, Item 1A: Risk Factors`, `Item 2.02`). `section` reads one, `find`
    returns matching passages with their offsets, and `offset` pages from any
    point, 20,000 characters at a time by default.
  - **Limits.** Images are counted, and the response says their text is not
    extracted. Only `https://www.sec.gov/Archives/edgar/data/` URLs are
    fetched, documents are capped at 25 MB, and recent documents are cached so
    paging fetches once.
  - **Dependency.** It adds html5ever's tokenizer: 10 crates, all pure Rust.
  - **Design.** The design and the prototype measurements are in
    `docs/filing-text-design.md`.
  - **Measured.** On the benchmark's 50 public questions it raised correct
    answers from 11 to 31, with none of the 11 lost
    (`evaluation/runs/2026-10-26-filing-text/`).

## [0.7.0] - 2026-10-26

### Added
- **Every tool accepts a CIK where it takes a ticker.** SEC's ticker list holds
  only current tickers, so a company that was acquired, went bankrupt or
  changed its ticker dropped out of reach, although its filings stay in EDGAR.
  US Steel (`X`, taken private by Nippon Steel) and Spirit Airlines (`SAVE`)
  were two such companies. The `ticker` field now also takes a CIK, written as
  `1163302`, `0001163302` or `CIK0001163302`. `sec_lookup_cik` given a CIK
  returns the company's name and current tickers, and a ticker that is not
  found now says to pass the CIK instead. The October 2026 Finance Agent
  Benchmark baseline lost four questions to this.

### Changed
- **`sec_company_facts` pages, filters and slims its listing.** It returned
  every concept in every taxonomy with its full XBRL description: 181k
  characters for Netflix, past what an MCP client will pass to a model, so the
  benchmark baseline lost five calls to it. Each row is now the concept, its
  label and `latest`, the most recent period end with data, which tells a
  current concept from one the company stopped using. `query` filters by name
  or label, `limit` (default 100, max 300) and `offset` page, and
  `include_descriptions` brings the descriptions back. **The response shape
  changed**: a flat `concepts` list with a `taxonomy` on each row replaces the
  `taxonomies` object.
- **`sec_insider_transactions` reads at most 10 filings per call** (was 20) and
  pages back with `offset`/`next_offset`. Twenty Form 4s came to 114k
  characters.
- **`sec_xbrl_frames` caps `limit` at 300** (it had no cap) and pages with
  `offset`/`next_offset`, reporting how many filers the frame holds in `total`.

## [0.6.2] - 2026-10-26

### Changed
- **rmcp 3.4.0 → 3.7.0.** No code changes were needed. The stdio-relevant
  fixes come along: a bounded incoming-line buffer, pending requests cancelled
  at EOF, and malformed JSON-RPC rejected. Builds with `--locked`, Glama's
  among them, were still on 3.4.0.
- **The listings say it is written in Rust.** The README's opening line and
  the `server.json` description, which the MCP registry shows, both now say
  so. The README adds that it runs as a single native binary with no Node,
  Python or Docker runtime.
- **`sec_lookup_cik` says how it behaves.** Its description was one sentence.
  It now covers how the ticker is matched (exactly, ignoring case, against
  SEC's active-ticker list, cached for an hour), what comes back (one 10-digit
  CIK; share classes such as GOOG and GOOGL share it), and that an unknown or
  delisted ticker is an error that `sec_list_tickers` can resolve by company
  name. Glama's tool-quality grader rated it the weakest tool in 0.6.1 for
  leaving exactly those questions open.

## [0.6.1] - 2026-10-24

### Added
- **Tool titles and behaviour annotations.** Every tool now carries a
  human-readable `title` and MCP `annotations`. The nine query tools are marked
  `readOnlyHint: true` and `openWorldHint: true`, since they only read from
  EDGAR. `sec_configure` is marked `readOnlyHint: false`,
  `destructiveHint: false`, `idempotentHint: true` and `openWorldHint: false`,
  because it writes only the local config file. Clients use these hints to
  decide what needs confirmation, and directories grade tools on them: the
  Claude connector directory requires `readOnlyHint` or `destructiveHint` on
  every tool, and Glama's tool-quality score marked every sec-mcp tool down
  for having none. A unit test and the offline stdio test both fail if a tool
  is added without them.

## [0.6.0] - 2026-10-23

### Added
- **Paging for `sec_financial_concept` and `sec_list_tickers`.** Both had
  fixed caps (the latest 20 facts, the first 50 tickers) and no way past them.
  Each now takes `limit` and `offset` and reports `returned` and, while more
  remain, `next_offset`, the same shape as `sec_full_text_search`. Defaults
  are unchanged, so existing calls return what they did before.

  `sec_financial_concept` pages **newest first**: offset 0 is the latest
  periods, and each later page reaches further back. Within a page, facts stay
  in chronological order. "Newest" means period end, which is how EDGAR sorts
  the facts; it is not filing date, since every 10-K re-reports earlier years
  as comparatives. `total_facts` gives the count after any period filter (max
  `limit` 100). `sec_list_tickers` adds `matched`, the number of tickers that
  fit the query, alongside the existing `total_tickers` (max `limit` 200).

### Changed
- **`sec_full_text_search` no longer returns amendments for a base-form
  filter.** This changes the default results, which is why this release is
  0.6.0. EDGAR matches `forms` against each hit's root form, so `forms:
  "10-K"` also returned `10-K/A`s — often partial documents (only Part III, or
  one exhibit) or restatements, easily mistaken for the annual report itself.
  Amendments are now excluded unless asked for, matching the period filters
  elsewhere in sec-mcp: request `10-K/A` directly, list both forms, or pass the
  new `include_amendments: true`. Any amendment returned carries
  `amendment: true`.

  The filtering happens after EDGAR's, so EDGAR's `total` still counts the
  dropped amendments. It is flagged `total_is_upper_bound`, or
  `total_is_approximate` when the count has also saturated at 10000, and each
  page reports `amendments_excluded`. A page can now return fewer than `limit`
  hits, so the response carries `next_offset`, which is the only correct place
  to resume — `offset + returned` would revisit skipped hits.

### Fixed
- **A Form 3 from an insider who owns nothing no longer looks like a parse
  failure.** A newly appointed officer files a Form 3 with empty holdings tables
  and `<noSecuritiesOwned>1</noSecuritiesOwned>`; `sec_insider_transactions`
  ignored the flag, so the result was indistinguishable from a document it
  failed to read. It now comes back as `no_securities_owned: true`. Found by the
  live smoke test on an NVDA Form 3 filed 3 September 2026.

### Security
- Bumped the TLS stack in `Cargo.lock` to clear RUSTSEC-2026-0346, -0347,
  -0348, -0349 (rustls 0.23.46), RUSTSEC-2026-0382 (rustls-webpki 0.103.16) and
  RUSTSEC-2026-0383 (webpki-root-certs 1.0.10). The last two concern
  distrusted emSign intermediates being accepted during certificate
  verification.
- Bumped tokio 1.52.3 → 1.54.1 in `Cargo.lock` to clear RUSTSEC-2026-0386
  (`AsyncFd::new` violates I/O safety). sec-mcp does not use `AsyncFd`.

## [0.5.2] - 2026-09-18

### Added
- **`sec_full_text_search` — search the body text of every filing since 2001,
  exhibits included.** Every other tool here needs you to already know the
  company; this one finds companies by what they *said* ("which filers discussed
  a climate transition plan in a 10-K"), which is a different question than
  sec-mcp could previously answer at all. Filter by form, ticker and date range;
  page with `offset`. Each hit is returned with a direct URL to the matching
  document, built from the hit id — a hit is otherwise a dead end, since the id
  names a document but not its location.

  Two shaping decisions worth recording. The raw `_source` carries film numbers,
  file numbers, SIC codes, XSL paths and a sequence number that answer nothing a
  caller asked, so the projection keeps only who filed, what, when and where to
  read it; 8-K `items` are kept, because they are the reason an 8-K exists. And
  `hits.total` saturates at 10000 with `relation: "gte"` — reporting that as an
  exact count would be a quiet lie, so it comes back flagged as
  `total_is_lower_bound`.

  **This is the one endpoint sec-mcp calls that SEC does not document.** It is
  the API behind the EDGAR full-text search UI, on `efts.sec.gov` rather than
  `data.sec.gov`, and it is absent from the published APIs page, so it carries
  no stability guarantee. That is a deliberate, bounded risk: `live_smoke` now
  asserts on its response shape, so drift surfaces as a failing test rather than
  as a confusing empty result.
- **Unit tests in `src/main.rs`** — the crate had none, only integration tests.
  Eight cover the new pure helpers: hit-id splitting (including ids that cannot
  be addressed), ticker extraction from a `display_names` entry (including
  filers that have no ticker, where the CIK parenthetical must not be mistaken
  for one), and the projection itself.

### Changed
- `protocol_surface` now asserts tool **names**, not just the count. A count
  catches a tool vanishing but not one being renamed, and the name is what
  clients bind to.
- README lists `sec_full_text_search` and `sec_insider_transactions`; the latter
  shipped in 0.5.1 without being added to the table.

## [0.5.1] - 2026-09-18

### Added
- **`sec_insider_transactions` — SEC Forms 3, 4 and 5.** Who bought or sold,
  when, how many shares and at what price, with each insider's name and role.
  Forms 3/4/5 were already *visible* (they are indexed under the issuer's CIK, so
  `sec_recent_filings` with `form_type: "4"` lists them) but the tool returned a
  URL the model could not read; this parses the filings themselves.

  Two pieces of shaping do most of the work. SEC transaction codes are expanded
  into plain language — a bare `F` reads like a sale when it actually means
  shares the issuer withheld to cover tax on vesting — and the footnotes each
  transaction references are resolved inline, which is where filers put the
  context that distinguishes a Rule 10b5-1 sale from an automatic settlement.

  Form 3 reports *initial holdings* rather than transactions, so results carry
  both `transactions` and `holdings`; a Form 3 is never an empty result.

  Takes `ticker`, an optional `form_type` (`3`/`4`/`5`), and `limit` (default 5,
  max 20). Each filing costs one request, issued sequentially, well inside SEC's
  10/sec fair-access limit. One unreadable document is reported in place rather
  than failing the whole call.
- `roxmltree` — the crate's first XML dependency, needed because ownership forms
  are XML rather than JSON. Its only required dependency, `memchr`, was already
  in the tree.
- When a period filter matches none of a concept's facts, the result carries
  `available_forms` and a `note` naming the forms the facts are actually filed
  on. This turns a silent empty result into a self-correcting one, and covers
  the ordinary case too — asking Apple for `Revenues` quarterly now reports that
  the concept is only tagged on 10-K.
- `docs/roadmap.md`, recording candidate EDGAR capabilities that are currently
  out of scope.

### Fixed
- **`sec_financial_concept`'s period filter returned nothing for foreign private
  issuers.** XBRL is not a US-domestic-only dataset — the SEC extracts facts from
  "10-Q, 10-K, 8-K, 20-F, 40-F, 6-K, and their variants" — but the filter matched
  `10-K` and `10-Q` exactly. A foreign filer reporting on `20-F`/`40-F`/`6-K` was
  therefore filtered down to an empty set, which reads as "this company reports
  nothing" rather than "wrong filter". `annual` now also matches `20-F`/`40-F`
  and `quarterly` also matches `6-K`. Measured on Novartis (295 `ifrs-full`
  concepts, filed on 6-K): `quarterly` returned 0 rows before, 3 after. Domestic
  filers are unaffected — amendments (`10-K/A`) stay excluded, as before, so a
  restated period is not counted twice.

### Security
- **rustls 0.23.40 → 0.23.45** (lockfile only), clearing RUSTSEC-2026-0285: TLS
  1.3 handshake messages were accepted across encryption-level boundaries. Note
  that `cargo update -p rustls` resolves only as far as 0.23.43, which is still
  affected — the bump needs `--precise`. The weekly `audit` workflow that would
  normally have caught this has not fired since Monday 7 September; the advisory
  was found by hand.

### Changed
- **rmcp 3.0.0 → 3.4.0.** This corrects protocol negotiation: a client naming
  `2026-07-28` over `initialize` is now answered with `2025-11-25` rather than
  having `2026-07-28` echoed back. That is the right answer — `2026-07-28`
  replaced the handshake with per-request `_meta`, so there is no handshake in
  which to agree on it, and the server must fall back to its newest revision
  that still has one. `protocol_surface` asserted the old behaviour and has been
  updated; the stateless path (`server/discover` with `_meta`) is unaffected, so
  2026-07-28 support itself is intact.
- `ServerInfo` → `ServerConfig`, the former now being a deprecated alias in rmcp.

## [0.5.0] - 2026-07-29

### Added
- Support for MCP protocol revision **2026-07-28**. The server now implements
  `server/discover` (mandatory in that revision — previously it answered
  `-32601 method not found`), serves the stateless request path where a client
  skips the handshake and self-describes via `_meta`, tags results with
  `resultType`, and advertises SEP-2549 cache hints on `tools/list`
  (`ttlMs` 5 min, `cacheScope: private` — the listing reflects this machine's
  configuration state, so it is not shareable between users). Revisions back to
  `2024-11-05` continue to negotiate as before.
- An offline `protocol_surface` test covering all of the above. It needs no
  network, so `cargo test` now has real protocol coverage rather than skipping
  everything without `SEC_MCP_LIVE_EMAIL`.

### Changed
- **The JSON-RPC layer is now the [`rmcp`](https://crates.io/crates/rmcp) SDK
  rather than hand-rolled.** Framing, version negotiation, `server/discover`,
  and `resultType` come from the SDK, so future protocol revisions are a
  dependency bump instead of hand-written conformance work. Tool names,
  descriptions, input schemas, result shapes, and every EDGAR call are
  unchanged.
- Tool input schemas are derived from Rust types by `schemars` instead of being
  written by hand. Two incidental differences: the integer `limit` fields now
  also carry `format: uint64` / `minimum: 0` (a tightening — negative values
  were never meaningful and previously fell back to the default), and each
  schema carries a `$schema` key.
- `tools/list` returns tools in a deterministic (alphabetical) order, which the
  2026-07-28 revision recommends so clients can cache the listing.
- **MSRV is now 1.88** (from 1.86), matching `rmcp` 3's own requirement.

### Removed
- **The prebuilt Linux binary channel, and with it the `release` workflow.**
  Releases no longer attach an `x86_64-unknown-linux-musl` tarball or
  `SHA256SUMS`; `cargo install sec-mcp` is the only install path, and the README
  section documenting the download is gone. The workflow existed solely to build
  and attach that artefact, so nothing remained once it went — and its pinned
  1.86.0 toolchain could not have built this release in any case.

### Fixed
- The server identified itself as `rmcp`/its SDK version rather than `sec-mcp`
  in the handshake — latent before, and newly visible through `server/discover`.
- `initialize` requests missing the spec-required `capabilities` or `clientInfo`
  are now rejected rather than silently accepted. Every real MCP client sends
  both; only the crate's own smoke test relied on the old leniency.

## [0.4.4] - 2026-07-21

### Changed
- Requests now declare `Accept-Encoding: gzip` (reqwest's `gzip` feature), which
  SEC's fair-access guidance asks callers to do and which the server had not
  been doing. Measured on `companyfacts` for CIK 0000320193: 3,748,682 bytes
  uncompressed vs 268,153 gzipped — a 14× reduction on the largest response the
  server fetches. Decoding is transparent, so no response handling changed.
- The convenience `edgar_url`/`edgar_page` links returned by `sec_lookup_cik`
  and `sec_company_info` now point at `sec.gov/edgar/browse/?CIK=`, the modern
  EDGAR company page. The previous `cgi-bin/browse-edgar?action=getcompany`
  form still works but 301-redirects there; nothing fetched these links, so
  this only saves the user a hop.

### Removed
- The prebuilt-binary install option (README "Option 1"). Install is now
  `cargo install sec-mcp` from crates.io only. An unsigned/un-notarised binary
  is a worse experience than building from source (macOS Gatekeeper quarantines
  it), and anyone able to configure an MCP server can run `cargo install` — so
  the download channel added maintenance without serving a real gap.

### Added
- Prebuilt **Linux** binary channel: every release now carries a static
  `x86_64-unknown-linux-musl` build plus a `SHA256SUMS`, produced by CI — no C
  compiler, no Rust toolchain, and no particular glibc, so it's a real
  no-dependencies install on any Linux. Other platforms remain `cargo install`
  (see Removed for the retired unsigned-macOS download).
- Opt-in live smoke tests (`tests/live_smoke.rs`) that drive the built binary
  over stdio against the real EDGAR APIs, gated on `SEC_MCP_LIVE_EMAIL` and
  skipped by default. Covers `sec_lookup_cik`, `sec_company_facts`,
  `sec_recent_filings`, and both simple- and compound-unit `sec_xbrl_frames` —
  the last a regression guard for the 0.4.2 per-share (`USD/shares`) fix.

### Changed
- Minimum supported Rust version is now **1.86** (was 1.85). Transitive
  dependencies (`icu_*`, `idna_adapter`) in the pinned lockfile require rustc
  1.86, so a `--locked` build on 1.85 no longer compiles.

### Security
- Bumped `anyhow` to 1.0.103, clearing RUSTSEC-2026-0190 (an unsoundness in
  `Error::downcast_mut`, not exercised by this crate). Dependency advisories are
  now checked in CI via `cargo-deny` on every push, pull request, and weekly.

## [0.4.2] - 2026-07-05

### Fixed
- **`sec_xbrl_frames` returned no data for per-share (compound) units.** The unit
  path segment was percent-encoded — added in 0.4.1's URL-injection hardening
  (`fc19e17`) — so a compound unit like `USD/shares` became `USD%2Fshares` and the
  frames API `404`'d. Compound units use the SEC's `-per-` convention in the path
  (`USD-per-shares`), not an encoded slash; the slash is now mapped to `-per-`
  before encoding, so per-share concepts such as `EarningsPerShareBasic` work
  again. Simple units (`USD`, `shares`, `pure`) were never affected. The `unit`
  tool description now documents the compound form.
