# Changelog

All notable changes to `guise-choice` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.8]

### Changed
- Version unified with the guise family. No API change.

## [0.1.3] - 2026-08-07

### Changed
- Updated `Cargo.toml` authors metadata to exact Santh identity (`Santh <64453045+santhreal@users.noreply.github.com>`).

### Added
- Gap tests pinning fail-closed behavior for non-deterministic projection closures during selection passes and subnormal float weight handling.

## [0.1.2] - 2026-08-07

### Added
- Standard README/SPEC/CHANGELOG and gap/property test coverage.

## [0.1.1] - 2026-08-07

### Added
- Standard `package.metadata.santh.status = "alpha"` metadata verification in `Cargo.toml`.
- Technical specification `SPEC.md`, crate overview `README.md`, and `CHANGELOG.md`.
- Property test suite (`tests/property.rs`) using `proptest` to verify bounds safety, probability clamping, weighted index guarantees, and seed injectivity.
- Gap test suite (`tests/gap.rs`) pinning weight sum overflow handling, projection multi-evaluation invariants, multiset alphabet bias, and DNS safety.

### Fixed
- Updated repository and homepage URLs in `Cargo.toml`.
- Bumped crate version to `0.1.1`.

## [0.1.0] - 2026-03-15

### Added
- Initial release of `guise-choice` uniform sampling, weighted indexing, chance gates, and persona seed expansion.
