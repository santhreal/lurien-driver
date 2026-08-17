//! Profile import: cookies required, logins optional, corrupt sqlite fails.

use lurien::error::Error;
use lurien::import_profile;
use std::fs;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lurien-import-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp");
    dir
}

fn write_sqlite(path: &std::path::Path) {
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.resize(64, 0);
    fs::write(path, bytes).expect("sqlite stub");
}

#[test]
fn cookies_and_logins_round_trip() {
    let src = tmp("src");
    let dest = tmp("dest");
    write_sqlite(&src.join("cookies.sqlite"));
    fs::write(src.join("logins.json"), "{}").unwrap();
    fs::write(src.join("key4.db"), b"key").unwrap();
    fs::create_dir_all(src.join("storage/default")).unwrap();
    fs::write(src.join("storage/default/x"), b"ls").unwrap();

    let report = import_profile(&src, &dest).expect("import");
    assert!(report.cookies);
    assert!(report.logins);
    assert!(report.local_storage);
    assert!(dest.join("cookies.sqlite").exists());
    assert!(dest.join("logins.json").exists());
    assert!(dest.join("key4.db").exists());
    assert!(dest.join("storage/default/x").exists());
}

#[test]
fn missing_key4_warns_and_keeps_cookies() {
    let src = tmp("src-nologin");
    let dest = tmp("dest-nologin");
    write_sqlite(&src.join("cookies.sqlite"));
    fs::write(src.join("logins.json"), "{}").unwrap();

    let report = import_profile(&src, &dest).expect("import without key4");
    assert!(report.cookies);
    assert!(!report.logins);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("logins skipped") && w.contains("key4.db")));
    assert!(dest.join("cookies.sqlite").exists());
    assert!(!dest.join("logins.json").exists());
}

#[test]
fn corrupt_cookies_is_error() {
    let src = tmp("src-bad");
    let dest = tmp("dest-bad");
    fs::write(src.join("cookies.sqlite"), b"<html>not sqlite</html>").unwrap();
    let err = import_profile(&src, &dest).expect_err("corrupt cookies");
    assert!(matches!(err, Error::CookiesCorrupt { .. }));
    assert!(
        !dest.join("cookies.sqlite").exists() || fs::read(dest.join("cookies.sqlite")).is_err()
    );
}

#[test]
fn missing_cookies_is_error() {
    let src = tmp("src-empty");
    let dest = tmp("dest-empty");
    let err = import_profile(&src, &dest).expect_err("no cookies");
    assert!(matches!(err, Error::CookiesCorrupt { .. }));
}
