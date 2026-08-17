//! Copy a real Firefox profile into a lurien profile dir.
//!
//! Cookies, logins (`logins.json` + `key4.db`), and localStorage.
//! Not extensions. Not `cert9.db`.

use crate::error::{is_sqlite_file, path_string, Error};
use std::path::{Path, PathBuf};

/// Result of an import. Logins may have been skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Destination profile directory.
    pub dest: PathBuf,
    /// `cookies.sqlite` was copied.
    pub cookies: bool,
    /// Both `logins.json` and `key4.db` were copied.
    pub logins: bool,
    /// `storage/` and/or `webappsstore.sqlite` were copied.
    pub local_storage: bool,
    /// Human-readable warnings (missing logins, no storage).
    pub warnings: Vec<String>,
}

/// Files copied as a pair or not at all.
const LOGIN_FILES: &[&str] = &["logins.json", "key4.db"];
const COOKIE_SIDECARS: &[&str] = &["cookies.sqlite-wal", "cookies.sqlite-shm"];

/// Import `src` into `dest`.
///
/// `cookies.sqlite` is required and must be a SQLite file. Missing logins
/// warn and continue. A locked `dest` is an error.
pub fn import_profile(src: &Path, dest: &Path) -> Result<ImportReport, Error> {
    if !src.is_dir() {
        return Err(Error::Other(format!(
            "profile source is not a directory: {}",
            path_string(src)
        )));
    }
    if dest.exists() && crate::error::profile_looks_locked(dest) {
        return Err(Error::ProfileLocked {
            path: path_string(dest),
        });
    }
    std::fs::create_dir_all(dest)
        .map_err(|e| Error::Other(format!("create profile dest {}: {e}", path_string(dest))))?;

    let cookies_src = src.join("cookies.sqlite");
    if !cookies_src.exists() {
        return Err(Error::CookiesCorrupt {
            path: path_string(&cookies_src),
            detail: "file missing".into(),
        });
    }
    if !is_sqlite_file(&cookies_src) {
        return Err(Error::CookiesCorrupt {
            path: path_string(&cookies_src),
            detail: "not a SQLite database".into(),
        });
    }
    copy_file(&cookies_src, &dest.join("cookies.sqlite"))?;
    for side in COOKIE_SIDECARS {
        let p = src.join(side);
        if p.exists() {
            copy_file(&p, &dest.join(side))?;
        }
    }

    let mut warnings = Vec::new();
    let logins = import_logins(src, dest, &mut warnings)?;
    let local_storage = import_storage(src, dest, &mut warnings)?;

    Ok(ImportReport {
        dest: dest.to_path_buf(),
        cookies: true,
        logins,
        local_storage,
        warnings,
    })
}

fn import_logins(src: &Path, dest: &Path, warnings: &mut Vec<String>) -> Result<bool, Error> {
    let present: Vec<_> = LOGIN_FILES
        .iter()
        .copied()
        .filter(|n| src.join(n).exists())
        .collect();
    if present.len() == LOGIN_FILES.len() {
        for name in LOGIN_FILES {
            copy_file(&src.join(name), &dest.join(name))?;
        }
        return Ok(true);
    }
    if present.is_empty() {
        warnings.push(
            "logins skipped: logins.json and key4.db missing. cookies and localStorage imported."
                .into(),
        );
    } else {
        warnings.push(format!(
            "logins skipped: need both logins.json and key4.db (found {}). Do not invent passwords.",
            present.join(", ")
        ));
    }
    Ok(false)
}

fn import_storage(src: &Path, dest: &Path, warnings: &mut Vec<String>) -> Result<bool, Error> {
    let mut any = false;
    let storage = src.join("storage");
    if storage.is_dir() {
        copy_dir(&storage, &dest.join("storage"))?;
        any = true;
    }
    let webapps = src.join("webappsstore.sqlite");
    if webapps.exists() {
        if !is_sqlite_file(&webapps) {
            return Err(Error::Other(format!(
                "corrupt webappsstore.sqlite at {}",
                path_string(&webapps)
            )));
        }
        copy_file(&webapps, &dest.join("webappsstore.sqlite"))?;
        any = true;
    }
    if !any {
        warnings.push("no localStorage found (storage/ or webappsstore.sqlite)".into());
    }
    Ok(any)
}

fn copy_file(src: &Path, dest: &Path) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Other(format!("mkdir {}: {e}", path_string(parent))))?;
    }
    std::fs::copy(src, dest).map_err(|e| {
        Error::Other(format!(
            "copy {} -> {}: {e}",
            path_string(src),
            path_string(dest)
        ))
    })?;
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest)
        .map_err(|e| Error::Other(format!("mkdir {}: {e}", path_string(dest))))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| Error::Other(format!("read {}: {e}", path_string(src))))?
    {
        let entry = entry.map_err(|e| Error::Other(format!("readdir: {e}")))?;
        let ty = entry
            .file_type()
            .map_err(|e| Error::Other(format!("stat: {e}")))?;
        let to = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if ty.is_file() {
            copy_file(&entry.path(), &to)?;
        }
    }
    Ok(())
}
