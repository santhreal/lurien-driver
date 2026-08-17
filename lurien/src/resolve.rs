//! Engine binary resolution. Missing binary is `Err`. Never `/usr/bin/firefox`.

use crate::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Resolve `LURIEN_BIN` (then one-release aliases, then install paths).
pub fn resolve_engine() -> Result<String, Error> {
    guise::browser::resolve_lurien_bin().map_err(Error::from_resolve)
}

/// Same as [`resolve_engine`], then refuse a non-executable or non-Firefox path.
pub fn resolve_engine_checked() -> Result<String, Error> {
    let path = resolve_engine()?;
    check_engine(&path)?;
    Ok(path)
}

/// Executable Firefox-family binary, or a typed error naming `file(1)`.
pub fn check_engine(path: &str) -> Result<(), Error> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|_| Error::EngineNotExecutable {
        path: path.to_string(),
    })?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(Error::EngineNotExecutable {
            path: path.to_string(),
        });
    }
    if looks_like_stock_firefox(path) {
        return Err(Error::NotFirefox {
            path: path.to_string(),
            hint: " lurien never falls back to /usr/bin/firefox.".to_string(),
        });
    }
    match crate::version::engine_version_string(path) {
        Some(v) if version_is_firefox_family(&v) => Ok(()),
        Some(v) => Err(Error::NotFirefox {
            path: path.to_string(),
            hint: format!(" --version was {v:?}. lurien never falls back to /usr/bin/firefox."),
        }),
        None => Err(Error::NotFirefox {
            path: path.to_string(),
            hint: format!(" --version unread. Check with: file {path}"),
        }),
    }
}

fn looks_like_stock_firefox(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let stock = lower.ends_with("/firefox") || lower.ends_with("/firefox-bin");
    let ours = lower.contains("lurien") || lower.contains("reynard") || lower.contains("camoufox");
    stock && !ours
}

fn version_is_firefox_family(version: &str) -> bool {
    let lower = version.to_ascii_lowercase();
    lower.contains("firefox") || lower.contains("camoufox") || lower.contains("lurien")
}
