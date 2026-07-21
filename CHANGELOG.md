# Changelog

All notable changes to sec-mcp are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.4.2 are recorded only in the git tags (`v0.1.0`–`v0.4.1`).

## [Unreleased]

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
