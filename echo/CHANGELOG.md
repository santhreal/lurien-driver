# Changelog

All notable changes to `guise-echo` will be documented in this file.

## [0.1.5] - 2026-08-07
### Fixed
- **H2 Control Frame Length Validation**: Removed early zero-length frame skip in `read_h2_preface_and_frames` that allowed empty `WINDOW_UPDATE` and `PRIORITY` frames to bypass RFC 9113 payload length requirements.
- **H2 Frame Capture Error Surface**: Added optional `error` field to `H2ConnectionInfo` to surface H2 preface and frame parse errors in JSON response instead of silently swallowing them.
- **RFC 9113 H2 Invariants**: Enforced parameter ID uniqueness in `SETTINGS` frames and prohibited self-dependent stream dependencies in `PRIORITY` frames.
- **RFC 8446 TLS ClientHello Invariants**: Enforced single Handshake message constraint per record payload and prohibited duplicate extension types in ClientHello extension blocks.

## [0.1.4] - 2026-08-07

### Fixed
- **H2 Frame Capture Loop**: Fixed early `continue` in `read_h2_preface_and_frames` that caused empty request-bearing frames (`DATA`, `HEADERS`, `CONTINUATION` with length 0) to bypass loop termination.
- **RFC 9113 H2 Validation**: Enforced zero payload length for `SETTINGS` ACK frames, zero stream ID for `SETTINGS` frames, and non-zero stream ID for `PRIORITY` frames.
- **TLS Extension Bounds**: Enforced strict completeness check on `tls_parser::parse_tls_extensions` to fail closed on trailing unparsed bytes in extension blocks.
- **Linux TCP Sysctl Diagnostics**: Updated `read_bool_proc_sys` in `src/tcp.rs` to recognize Linux sysctl setting value `2` (RFC 7323 timestamps with per-route/listener flags) as enabled (`Some(true)`).

### Changed
- **Metadata Alignment**: Set `authors` in `Cargo.toml` to `["Santh <64453045+santhreal@users.noreply.github.com>"]`.

## [0.1.3] - 2026-08-07

### Fixed
- **Adversarial test assertion fix**: Corrected `listener_survives_hostile_connection` assertion in `tests/adversarial.rs` where `response.windows(2).any(|w| w == b"{")` compared a 2-byte slice against a 1-byte pattern, causing false test failures despite valid server JSON output.

### Changed
- **Metadata Alignment**: Updated `Cargo.toml` with `package.metadata.santh` status (`beta`), repository/homepage URLs, and documentation URL.
- **Documentation**: Added comprehensive `README.md` and technical `SPEC.md` matching Santh monorepo standards.
- **Module Documentation**: Clarified TCP diagnostics documentation in `src/lib.rs`.
