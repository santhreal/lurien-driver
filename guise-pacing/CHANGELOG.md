# Changelog - guise-pacing

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.8]

### Changed
- Version unified with the guise family. No API change.

## [0.1.4] - 2026-08-07
### Fixed
- `BoundedNormalDelay`: Lowered Box-Muller $u_1$ sampling bound from `0.001` to `f64::EPSILON`, resolving artificial distribution truncation that prevented sampling the outer ~3.5% tail of configured delay envelopes near `min_ms` and `max_ms`.
- `capped_pow2_backoff`: Ensured base duration nanos are capped against `effective_max` (`max.min(MAX_PACING_BACKOFF)`) from step 1.

### Added
- `BackoffPolicy` & `RequestPacer`: Added `rate_limited_base_ms()`, `timeout_base_ms()`, and `base()` inspectability getters to complete public inspection contracts.

## [0.1.3] - 2026-08-07

### Fixed
- `parse_retry_after`: Digit strings exceeding `u64::MAX` now saturate to `u64::MAX` and clamp to `MAX_RETRY_AFTER_OBEYED` (60s) instead of failing to parse and returning `None` (silent fallback to zero-delay retries).
- `capped_exponential_backoff`: Delay calculation now clamps `max_ms` to `MAX_PACING_BACKOFF` (60s), ensuring pathological or multi-minute configured caps honor the fleet pacing ceiling.

### Changed
- Updated package metadata authors to `Santh <64453045+santhreal@users.noreply.github.com>`.

## [0.1.2] - 2026-08-07

### Added
- Standard metadata, status declaration (`alpha`), and comprehensive documentation (`README.md`, `SPEC.md`, `CHANGELOG.md`).

### Fixed
- Updated crate version from `0.1.1` to `0.1.2` for release tracking against crates.io.

## [0.1.1] - 2026-07-17

### Added
- `RequestPacer` inter-request pacing engine with adaptive challenge penalty handling for 429/403 signals.
- `BoundedNormalDelay` Box-Muller distribution envelope for human-like request timing.
- `parse_retry_after` HTTP response header parsing for delta-seconds and HTTP-date formats with 60s obedience cap.
- `jittered_backoff` deterministic nonce-keyed backoff jittering.

## [0.1.0] - 2026-06-09

### Added
- Initial release of `guise-pacing`: `Jitter`, `BackoffPolicy`, `capped_pow2_backoff`, `capped_exponential_backoff`, and `percent_jitter`.
