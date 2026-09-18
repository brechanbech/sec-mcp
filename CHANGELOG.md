# Changelog

All notable changes to sec-mcp are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.4.2 are recorded only in the git tags (`v0.1.0`–`v0.4.1`).

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
