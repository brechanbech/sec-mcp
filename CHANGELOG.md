# Changelog

All notable changes to sec-mcp are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.4.2 are recorded only in the git tags (`v0.1.0`–`v0.4.1`).

## [Unreleased]

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

### Added
- When a period filter matches none of a concept's facts, the result carries
  `available_forms` and a `note` naming the forms the facts are actually filed
  on. This turns a silent empty result into a self-correcting one, and covers
  the ordinary case too — asking Apple for `Revenues` quarterly now reports that
  the concept is only tagged on 10-K.
- `docs/roadmap.md`, recording candidate EDGAR capabilities that are currently
  out of scope.

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
