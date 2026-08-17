//! Crate semver plus engine `--version`.

use crate::resolve::resolve_engine;
use std::process::Command;

/// `lurien-browser` Cargo package version.
#[must_use]
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `lurien --version` of the resolved engine binary.
#[must_use]
pub fn engine_version_string(bin: &str) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// One line: crate + engine. Engine missing is named, not silent.
#[must_use]
pub fn version_line() -> String {
    match resolve_engine() {
        Ok(bin) => match engine_version_string(&bin) {
            Some(v) => format!(
                "lurien-browser {crate} / engine {v}",
                crate = crate_version()
            ),
            None => format!(
                "lurien-browser {crate} / engine {bin} (--version unread)",
                crate = crate_version()
            ),
        },
        Err(_) => format!(
            "lurien-browser {crate} / engine missing. Run install.sh or set LURIEN_BIN.",
            crate = crate_version()
        ),
    }
}
