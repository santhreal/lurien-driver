//! Missing engine is Err. Reintroducing Option + Firefox fallback must go red.

use lurien::error::Error;
use lurien::resolve::{check_engine, resolve_engine};

#[test]
fn stock_firefox_path_is_refused() {
    // The product never falls back to /usr/bin/firefox even if that file exists.
    let err = check_engine("/usr/bin/firefox").expect_err("stock firefox is not lurien");
    match err {
        Error::EngineNotExecutable { path } | Error::NotFirefox { path, .. } => {
            assert!(path.contains("firefox"));
        }
        other => panic!("expected NotFirefox / not-executable, got {other:?}"),
    }
}

#[test]
fn non_firefox_binary_is_refused() {
    let err = check_engine("/usr/bin/true").expect_err("true is not lurien");
    match err {
        Error::NotFirefox { path, hint } => {
            assert!(path.contains("true"));
            assert!(hint.contains("--version") || hint.contains("file"));
        }
        other => panic!("expected NotFirefox, got {other:?}"),
    }
}

#[test]
fn resolve_engine_never_returns_empty() {
    // If the host has an install, we get a path; if not, EngineMissing.
    match resolve_engine() {
        Ok(p) => assert!(!p.trim().is_empty(), "resolved path must be non-empty"),
        Err(Error::EngineMissing) => {}
        Err(other) => panic!("unexpected resolve error: {other}"),
    }
}

#[test]
fn mutation_option_fallback_is_not_the_api() {
    // The public resolver is Result, not Option. A future edit that
    // reintroduces Option + Firefox fallback fails this type-level check.
    fn assert_result(_: fn() -> Result<String, Error>) {}
    assert_result(resolve_engine);
}
