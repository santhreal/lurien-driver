//! Downloads: where a file lands, and how a caller waits for it.
//!
//! A download is not a page event a caller can poll for. Firefox decides where
//! the file goes, whether to ask first, and when it is complete, so all of that
//! is settled at launch: one directory per session, no prompt, and the BiDi
//! download events that foxdriver already captures name the file when it is
//! finished. This module owns the directory, the prefs that point Firefox at it,
//! and the read side the verbs use, so nothing else in the tree decides what a
//! download means.

use crate::error::Error;
use crate::session::Session;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Poll interval while waiting for a download to finish.
const POLL_MS: u64 = 100;

/// MIME types Firefox saves without asking. A prompt in an unattended session is
/// a hang, so the common document and archive types are answered in advance.
const NEVER_ASK: &str = "application/octet-stream,application/pdf,application/zip,\
application/gzip,application/x-tar,application/json,application/msword,\
application/vnd.openxmlformats-officedocument.wordprocessingml.document,\
application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,\
text/plain,text/csv,text/html,image/png,image/jpeg,image/svg+xml";

/// A download directory for one session. `LURIEN_DOWNLOAD_DIR` names it
/// explicitly; otherwise each session gets its own, so two sessions downloading
/// `invoice.pdf` do not overwrite each other.
#[must_use]
pub fn session_dir() -> String {
    if let Some(dir) = std::env::var("LURIEN_DOWNLOAD_DIR")
        .ok()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
    {
        return dir;
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lurien-downloads-{}-{n}",
        std::process::id()
    ));
    dir.to_string_lossy().into_owned()
}

/// The prefs that make a download land in `dir` with no prompt. Written into the
/// profile's `user.js` at launch, because a pref set after startup would miss a
/// download the first page starts.
#[must_use]
pub fn prefs(dir: &Path) -> String {
    let dir = guise::browser::escape_pref_value(&dir.to_string_lossy());
    [
        // 2 means "the directory named below", rather than Desktop or the OS default.
        r#"user_pref("browser.download.folderList", 2);"#.to_string(),
        format!(r#"user_pref("browser.download.dir", "{dir}");"#),
        r#"user_pref("browser.download.useDownloadDir", true);"#.to_string(),
        r#"user_pref("browser.download.start_downloads_in_tmp_dir", false);"#.to_string(),
        r#"user_pref("browser.download.always_ask_before_handling_new_types", false);"#.to_string(),
        r#"user_pref("browser.download.alwaysOpenPanel", false);"#.to_string(),
        r#"user_pref("browser.download.manager.showWhenStarting", false);"#.to_string(),
        format!(r#"user_pref("browser.helperApps.neverAsk.saveToDisk", "{NEVER_ASK}");"#),
        // Without this a PDF opens in the viewer instead of arriving as a file.
        r#"user_pref("pdfjs.disabled", true);"#.to_string(),
    ]
    .join("\n")
}

/// Create the directory a session downloads into. A download that cannot land is
/// a silent loss otherwise: Firefox reports the download as started and the file
/// never appears.
pub fn ensure_dir(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::DownloadDirUnusable {
        path: dir.to_string_lossy().into_owned(),
        detail: e.to_string(),
    })
}

/// Where this session's downloads land.
#[must_use]
pub fn dir_of(session: &Session) -> PathBuf {
    PathBuf::from(
        session
            .options()
            .download_dir
            .clone()
            .unwrap_or_else(session_dir),
    )
}

/// One download as a caller sees it: what the page called it, where it landed,
/// and whether the bytes are actually on disk yet.
#[must_use]
pub fn row(download: &runtime_foxdriver::dialog::CapturedDownload) -> Value {
    let size = download
        .filepath
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len());
    json!({
        "file": download.suggested_filename,
        "url": download.url,
        "status": download.status,
        "path": download.filepath,
        "size_bytes": size,
        "on_disk": size.is_some(),
    })
}

/// Every download this session has seen, oldest first.
pub async fn list(session: &Session) -> Result<Vec<Value>, Error> {
    let telemetry = session.telemetry().await?;
    Ok(telemetry
        .dialogs
        .downloads()
        .await
        .iter()
        .map(row)
        .collect())
}

/// Wait until a download whose filename contains `name` has finished, or until
/// `timeout_ms` elapses. With no `name`, the most recent finished download wins.
///
/// A completed BiDi event is not proof the bytes are readable, so the file must
/// exist on disk before this returns.
pub async fn wait(
    session: &Session,
    name: Option<&str>,
    timeout_ms: u64,
) -> Result<Value, Error> {
    let telemetry = session.telemetry().await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let downloads = telemetry.dialogs.downloads().await;
        let seen: Vec<String> = downloads
            .iter()
            .map(|d| format!("{} ({})", d.suggested_filename, d.status))
            .collect();
        if let Some(done) = downloads
            .iter()
            .rev()
            .filter(|d| matches(d, name))
            .find(|d| d.status == "complete" && on_disk(d))
        {
            return Ok(row(done));
        }
        if let Some(canceled) = downloads.iter().rev().find(|d| {
            matches(d, name) && d.status == "canceled"
        }) {
            return Err(Error::DownloadFailed {
                file: canceled.suggested_filename.clone(),
                detail: "the browser canceled it".to_string(),
            });
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::DownloadFailed {
                file: name.unwrap_or("any").to_string(),
                detail: format!(
                    "nothing finished within {timeout_ms}ms; seen so far: {}",
                    if seen.is_empty() {
                        "nothing".to_string()
                    } else {
                        seen.join(", ")
                    }
                ),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
    }
}

/// Copy a finished download to `dest`, returning the destination and its size.
/// Waiting is part of saving: a caller that asks for the file wants the bytes,
/// not a race with the browser.
pub async fn save(
    session: &Session,
    name: Option<&str>,
    dest: &Path,
    timeout_ms: u64,
) -> Result<Value, Error> {
    let done = wait(session, name, timeout_ms).await?;
    let source = done["path"].as_str().ok_or_else(|| Error::DownloadFailed {
        file: name.unwrap_or("any").to_string(),
        detail: "the browser reported no path for it".to_string(),
    })?;
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            ensure_dir(parent)?;
        }
    }
    let bytes = std::fs::copy(source, dest).map_err(|e| Error::DownloadFailed {
        file: done["file"].as_str().unwrap_or("download").to_string(),
        detail: format!("copy to {}: {e}", dest.display()),
    })?;
    Ok(json!({
        "file": done["file"],
        "url": done["url"],
        "source": source,
        "saved_to": dest.to_string_lossy(),
        "size_bytes": bytes,
    }))
}

fn matches(download: &runtime_foxdriver::dialog::CapturedDownload, name: Option<&str>) -> bool {
    match name {
        None => true,
        Some(name) => {
            let name = name.to_lowercase();
            download.suggested_filename.to_lowercase().contains(&name)
                || download
                    .filepath
                    .as_deref()
                    .is_some_and(|p| p.to_lowercase().contains(&name))
        }
    }
}

fn on_disk(download: &runtime_foxdriver::dialog::CapturedDownload) -> bool {
    download
        .filepath
        .as_deref()
        .is_some_and(|p| Path::new(p).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(name: &str, status: &str, path: Option<&str>) -> runtime_foxdriver::dialog::CapturedDownload {
        runtime_foxdriver::dialog::CapturedDownload {
            context: "ctx".into(),
            suggested_filename: name.into(),
            url: format!("https://example.test/{name}"),
            status: status.into(),
            filepath: path.map(str::to_string),
        }
    }

    /// A download directory that Firefox cannot parse is a download that vanishes:
    /// the pref is dropped, the file goes to the real Downloads folder, and the
    /// session reports a path nothing wrote.
    #[test]
    fn a_path_with_quotes_or_newlines_stays_one_pref_line() {
        let prefs = prefs(Path::new("/tmp/we\"ird\npath"));
        for line in prefs.lines() {
            assert!(
                line.starts_with("user_pref(") && line.ends_with(");"),
                "pref split across lines: {line}"
            );
        }
        assert!(prefs.contains(r#"/tmp/we\"ird\npath"#), "{prefs}");
    }

    /// Every pref here exists to stop a prompt or a redirect of the bytes. Losing
    /// one turns an unattended download into a hang or a file in the wrong place.
    #[test]
    fn the_prefs_pin_the_directory_and_answer_the_prompt() {
        let prefs = prefs(Path::new("/tmp/dl"));
        for pin in [
            r#"user_pref("browser.download.folderList", 2);"#,
            r#"user_pref("browser.download.dir", "/tmp/dl");"#,
            r#"user_pref("browser.download.useDownloadDir", true);"#,
            r#"user_pref("browser.download.always_ask_before_handling_new_types", false);"#,
            r#"user_pref("pdfjs.disabled", true);"#,
        ] {
            assert!(prefs.contains(pin), "missing {pin} in:\n{prefs}");
        }
        assert!(prefs.contains("application/pdf"), "pdf must save silently");
    }

    #[test]
    fn two_sessions_do_not_share_a_download_directory() {
        // The variable is a deliberate override: a caller that names a directory
        // gets exactly that one.
        assert_ne!(session_dir(), session_dir());
    }

    #[test]
    fn a_name_matches_the_suggested_filename_or_the_path_case_insensitively() {
        let d = captured("Invoice-2026.PDF", "complete", Some("/tmp/dl/Invoice-2026.PDF"));
        assert!(matches(&d, None));
        assert!(matches(&d, Some("invoice")));
        assert!(matches(&d, Some("/tmp/DL/")));
        assert!(!matches(&d, Some("receipt")));
    }

    /// The row is what a caller reads. A download the browser called complete but
    /// whose bytes are missing must not claim to be on disk.
    #[test]
    fn a_row_reports_bytes_only_when_the_file_is_there() {
        let missing = row(&captured("gone.bin", "complete", Some("/tmp/lurien-not-here.bin")));
        assert_eq!(missing["on_disk"], json!(false));
        assert_eq!(missing["size_bytes"], Value::Null);

        let dir = std::env::temp_dir().join(format!("lurien-dl-test-{}", std::process::id()));
        ensure_dir(&dir).expect("temp dir");
        let path = dir.join("here.bin");
        std::fs::write(&path, b"1234").expect("write");
        let there = row(&captured(
            "here.bin",
            "complete",
            Some(&path.to_string_lossy()),
        ));
        assert_eq!(there["on_disk"], json!(true));
        assert_eq!(there["size_bytes"], json!(4));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
