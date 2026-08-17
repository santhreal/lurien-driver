# Changelog - guise-profiles

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.5] - 2026-08-07
### Fixed
- Fixed User-Agent profile inference in `profile_from_user_agent_facts` to return `None` when platform is `UserAgentPlatform::Unknown` across all browser families (Edge, IE11, Opera, SamsungInternet), eliminating silent fallbacks to desktop Windows/Android profiles for unidentifiable platforms.
- Fixed mobile browser token detection and major version parsing in `user_agent_facts` for Edge (`EdgA/`, `EdgiOS/`), Chrome (`CriOS/`), and Opera (`OPiOS/`, `OPT/`), preventing browser misclassification and missing version facts.
- Fixed `get_profile` to resolve any catalog profile name or alias supported by `named_profile` (including `"chrome-windows"`, `"chrome-macos"`, `"firefox-windows"`, `"brave"`, `"opera"`, `"samsung-internet"`), returning its canonical `HeaderProfile` projection instead of `None`.
- Fixed `os_network_options_match` to normalize token whitespace during TCP option layout comparison, preventing false mismatches for space-padded option strings.
## [0.1.4] - 2026-08-07

### Fixed
- Fixed version number parsing in `major_after` to stop at any non-digit delimiter (e.g. `-`, `/`, `,`), preventing silent profile inference fallbacks to default profiles for versions with build/channel suffixes (e.g. `Chrome/96-legacy`).
- Fixed `get_profile` to normalize whitespace and ASCII casing on config profile names, resolving silent lookup failures for names like `"CHROME"` or `" chrome "`.
- Fixed `OsNetworkStack::ja4t` to trim whitespace around TCP option layout tokens, eliminating unnecessary JA4T rendering failures on layouts with spaces after commas.


## [0.1.3] - 2026-08-07

### Fixed
- Fixed silent fallbacks in User-Agent profile inference where unsupported browser/platform combinations (e.g. Firefox on iOS/Android, Safari on Windows/Linux, Edge/Opera on macOS) quietly returned mismatched desktop profiles instead of `None`.
- Fixed iOS platform detection to include `iPod`, `iPhone OS`, and `CPU OS` tokens, preventing iPod Touch UAs from misclassifying as macOS.
- Added Firefox on iOS (`FxiOS/`) token and version parsing support in `user_agent_facts`.
- Updated version parsing delimiter in `major_after` to support underscore-separated version strings.

### Changed
- Updated package author to `Santh <64453045+santhreal@users.noreply.github.com>`.
## [0.1.2] - 2026-08-07

### Added
- Standard metadata, status declaration (`beta`), and documentation (`README.md`, `SPEC.md`, `CHANGELOG.md`).
- Dedicated adversarial test suite (`tests/adversarial.rs`) testing User-Agent parser boundaries, header casing, named profile normalizations, and network TTL edge cases.

### Fixed
- Hardened compile-time hardware table non-emptiness check in `lib.rs` to dynamically iterate over `ALL_PROFILES` at compile time.

### Changed
- Standardized lint preamble in `lib.rs` with `clippy::pedantic` warnings and forbidden unsafe code.

## [0.1.1] - 2026-07-17

### Added
- Pure `os_network` transport-layer TCP/IP SYN fingerprint projections (`OsNetworkStack`, initial TTL de-hopping, p0f signature rendering, and JA4T fingerprint matching).
- `profile_platform` exact OS family mapping.

## [0.1.0] - 2026-06-01

### Added
- Initial release of `guise-profiles`: `StealthProfile` selector, `ProfileFacts`, catalog headers, User-Agent parser, and hardware display specs.
