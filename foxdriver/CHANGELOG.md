# Changelog

## [Unreleased]
### Fixed

- `launch_firefox_self_managed` waits for the remote agent to answer `session.status`, not merely for its debugging port to accept a connection. Gecko binds that socket seconds before the agent answers, and rustenium allows a hardcoded five seconds for `session.new` and panics past it, so a loaded host turned a browser that was starting normally into a launch failure. The wait is bounded at 60s and the failure names the address it asked. New `ready` module: a minimal WebSocket client for that one question, so this crate gains no WebSocket dependency.

### Changed

- Every public item is documented and `missing_docs` is now a deny. 127 items had
  accumulated behind the warning, most of them the captured network types, whose
  fields carry claims a caller cannot see from the name: a request body BiDi never
  delivers, a timing phase that is `None` on a plaintext connection, a base64 header
  value kept in that form, and counters that say whether a short log was a quiet page
  or a hit cap.
- `wait_until_ready` no longer initializes its last-failure string with a value it
  always overwrites.

## [0.1.5] - 2026-08-07
### Fixed

- `launch_firefox_self_managed` now nulls the child stdin/stdout/stderr. lurien-mcp is stdio JSON-RPC; inherited Gecko chatter on stdout is a protocol break.
- Normalized cookie domain strings in `Page::set_cookie` by stripping leading dots (`trim_start_matches('.')`) to prevent BiDi domain rejection errors.
- Added support for CDP `SameSite` string variants (`"no_restriction"`, `"no-restriction"`, `"no restriction"`) mapping to `SameSite::None` in `cookies::apply_to_page`.
- Added detection for existing JS function declarations in `Page::add_preload_script` to prevent double-wrapping function expressions into non-executing function returns.
- Extended `bidi_wire_value_to_json` and `remote_value_to_json` to support boolean object keys in JS object deserialization.
- Added file existence validation in `Page::set_files` to return a clear error before BiDi command execution.
- Added single-iframe `src="about:blank"` / empty-src fallback in `lookup_iframe_offset` for robust frame viewport offset calculations.
- Fixed process leak in `launch_firefox_self_managed` where self-managed child process was not reaped on port readiness timeout or BiDi attach failure error paths.
- Extended `remote_value_to_json` decoding to handle string-encoded numeric values (`"NaN"`, `"Infinity"`, `"-Infinity"`, `"-0"`), `RegExpRemoteValue`, and `DateRemoteValue`.
- Updated `lookup_iframe_offset` test assertions in `src/frame.rs` to match `Option<(f64, f64)>` fail-closed return semantics.
## [0.1.4] - 2026-08-07

### Changed
- Standardized `Cargo.toml` authors field to `Santh <64453045+santhreal@users.noreply.github.com>`.

### Fixed
- Fixed key resolution in WebDriver BiDi `ObjectRemoteValue` and wire-format object decoding (`bidi_wire_value_to_json` and `remote_value_to_json`) so object properties with wire-format key objects are preserved instead of silently dropped.
- Fixed case-sensitive matching in cookie `SameSite` attribute resolution (`cookies::apply_to_page`) so capitalized values ("Strict", "Lax", "None") are mapped properly instead of being dropped.
## [0.1.3] - 2026-08-07

### Added
- `SPEC.md` defining technical architecture, event stream invariants, and frame coordinate conversion guarantees.
- `README.md` and `package.metadata.santh` status badge and metadata (`beta`) in `Cargo.toml`.
- Folder contract alignment with `Cargo.lock` and standard `lib.rs` lint preamble (`missing_docs`, `clippy::pedantic`, `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::todo, clippy::unimplemented, clippy::panic))]`).

### Fixed
- Removed non-test `expect()` call in `browser.rs::launch_firefox` profile directory resolution path for full `#![cfg_attr(not(test), deny(clippy::expect_used))]` lint preamble compliance.

## [0.1.2] - 2026-08-02

### Fixed
- `to_curl` now single-quotes the captured HTTP method like every other captured value. Before this fix, a hostile server advertising a crafted method token could inject extra shell arguments into the generated curl command.
- New adversarial suite covering command substitution and backticks in captured values, single-quote breakout escaping, hostile method tokens, malformed JSON bodies, and malformed URLs.

## [0.1.1] - 2026-07-30

- Refined pass: metadata, docs, and test hardening for the crates.io release.
