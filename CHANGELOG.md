# Changelog

All notable changes to sec-mcp are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases before 0.4.2 are recorded only in the git tags (`v0.1.0`–`v0.4.1`).

## [Unreleased]

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
