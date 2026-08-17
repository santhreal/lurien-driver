# Changelog

All notable changes to `guise-oracle` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.1.8]

### Changed
- Version unified with the guise family. No API change.
- The manifest contract test asserts the shipped version has a changelog entry
  instead of pinning a literal version, which reddened on every release.

## [0.1.4] - 2026-08-16

### Changed
- `ThreeWaySurface::reynard_value` is now `lurien_value`. The engine is named
  lurien; the old field named a product that no longer exists.

## [0.1.3] - 2026-08-07

### Fixed
- `DriftReport::is_consistent()` now validates detailed per-probe outcomes against top-level counters, rejecting inconsistent reports.
- Added `Capture::is_consistent()` and `Capture::has_duplicate_surfaces()` for offline fixture integrity checks.

### Added
- `DifferentialReport::persona_divergences()` / `persona_divergence_count()`.
- `ThreeWayReport::all_agree()`.
- `FromStr`/`Display` helpers for severity/determinism/divergence kinds and `ProbeReport::severity_enum()`.

## [0.1.2] - 2026-08-07

### Added
- `ProbeOutcome::message()` helper method to retrieve payload details for non-pass probe outcomes.
- `ThreeWayReport::is_consistent()` method to validate structural bounds on 3-way reports.
- `ThreeWayReport::from_captures()` constructor for offline triangulation across stock, reynard, and JS disguise captures.

### Fixed
- Standardized `Cargo.toml` authors to exact `Santh <64453045+santhreal@users.noreply.github.com>`.
- Enforced symmetric maximum severity resolution in `Capture::diff()` to eliminate severity downgrade asymmetry between differential capture pairs.
## [0.1.1] - 2026-08-07

### Added
- Standard `package.metadata.santh.status = "beta"` metadata in `Cargo.toml`.
- Technical specification `SPEC.md` and `CHANGELOG.md`.
- `Severity::as_str()`, `Display` implementation for `Severity`, `ProbeOutcome::is_drift()`, and `ProbeOutcome::is_error()`.
- Offline `Capture::diff()` implementation to calculate `DifferentialReport` from captured browser fixtures without runtime dependencies.
- Consistency validation methods (`DifferentialReport::is_consistent()`, `DriftReport::is_consistent()`, `ThreeWayReport::agreed_count()`, `DifferentialReport::engine_worst()`).
- Comprehensive unit and contract test suites under `tests/unit/` and `tests/contract/`.

### Fixed
- Standardized `rust-version = "1.85"` and lint preamble in `src/lib.rs`.
- Bumped crate version to `0.1.1` for patch bump release on crates.io.

## [0.1.0] - 2026-03-15

### Added
- Initial release of `guise-oracle` taxonomy and report data types.
