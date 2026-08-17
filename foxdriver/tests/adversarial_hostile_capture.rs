//! Adversarial tests for foxdriver's handling of hostile captured data.
//!
//! Everything in a `NetworkEntry` ultimately comes from the wire: a hostile
//! server chooses response headers, redirect targets, and (within browser
//! limits) influences request metadata. When foxdriver renders that data
//! into a shell command (`to_curl`) or parses it (`json_body`,
//! `query_params`), hostile values must stay inert and malformed values
//! must fail loudly instead of panicking.

use runtime_foxdriver::{CapturedHeader, CapturedRequest, NetworkEntry};

/// Build a request with the given method/url/header/body for to_curl tests.
fn request_with(
    method: &str,
    url: &str,
    header: Option<(&str, &str)>,
    body: Option<&str>,
) -> NetworkEntry {
    NetworkEntry {
        request: CapturedRequest {
            id: "1".into(),
            context: None,
            method: method.into(),
            url: url.into(),
            headers: header
                .map(|(name, value)| {
                    vec![CapturedHeader {
                        name: name.into(),
                        value: value.into(),
                    }]
                })
                .unwrap_or_default(),
            post_data: body.map(Into::into),
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: Default::default(),
            cookies: vec![],
        },
        response: None,
        error: None,
    }
}

/// Why: `to_curl` emits a command line callers are expected to paste into
/// a shell. A captured value containing command substitution must survive
/// as inert literal text inside single quotes; if quoting ever breaks, the
/// "replay" helper becomes a remote code execution vector.
#[test]
fn to_curl_command_substitution_stays_inert() {
    let entry = request_with(
        "GET",
        "https://evil.example/$(touch /tmp/pwned)",
        Some(("X-Evil", "`id`")),
        Some("$(reboot)"),
    );
    let curl = entry.to_curl();

    // Every hostile fragment must appear inside single quotes, i.e. each
    // `$(` occurrence must be preceded somewhere on the line by an opening
    // quote that is not closed before it. The simplest robust check: the
    // exact substrings are present wrapped in single quotes.
    assert!(
        curl.contains("'https://evil.example/$(touch /tmp/pwned)'"),
        "hostile URL must be single-quoted, got: {curl}"
    );
    assert!(
        curl.contains("-H 'X-Evil: `id`'"),
        "hostile header must be single-quoted, got: {curl}"
    );
    assert!(
        curl.contains("-d '$(reboot)'"),
        "hostile body must be single-quoted, got: {curl}"
    );
}

/// Why: a single quote inside a captured value is the escape hatch for
/// shell injection. The `'\''` sequence must close, escape, and reopen the
/// quote so the value cannot break out.
#[test]
fn to_curl_single_quote_breakout_is_escaped() {
    let entry = request_with("GET", "https://a.example/'; rm -rf /; '", None, None);
    let curl = entry.to_curl();
    assert!(
        curl.contains("'https://a.example/'\\''; rm -rf /; '\\'''"),
        "embedded quote must be escaped as '\\'', got: {curl}"
    );
}

/// Why: the HTTP method is a captured string too. BiDi delivers it as text,
/// and a non-standard method containing spaces must not split into extra
/// command arguments. Before the fix, the method was interpolated
/// unquoted.
#[test]
fn to_curl_hostile_method_cannot_inject_arguments() {
    let entry = request_with(
        "POST http://evil.example/x",
        "https://a.example/",
        None,
        None,
    );
    let curl = entry.to_curl();
    assert!(
        curl.starts_with("curl -X 'POST http://evil.example/x' 'https://a.example/'"),
        "method must be a single quoted token, got: {curl}"
    );
}

/// Why: `json_body` parses server-controlled POST bodies. Malformed JSON
/// must yield `None` (documented "if it looks like JSON") rather than panic
/// or surface a partial parse.
#[test]
fn json_body_malformed_json_returns_none_not_panic() {
    for body in [
        "{not json",
        "{\"a\":}",
        "   ",
        "\u{0}\u{1}\u{2}",
        "{\"a\": 1,}",
    ] {
        let entry = request_with("POST", "https://a.example/", None, Some(body));
        assert!(
            entry.request.json_body().is_none(),
            "malformed body {body:?} must parse to None"
        );
    }

    let ok = request_with("POST", "https://a.example/", None, Some(r#"{"a":1}"#));
    assert_eq!(
        ok.request.json_body().unwrap()["a"],
        serde_json::json!(1),
        "valid JSON body must parse"
    );
}

/// Why: `query_params` parses the captured URL with the `url` crate.
/// Malformed URLs must return `Err` (the API is honest about failure)
/// instead of panicking or silently returning an empty param list.
#[test]
fn query_params_malformed_url_errors_not_empty() {
    let bad = request_with("GET", "http://[::1", None, None);
    assert!(
        bad.request.query_params().is_err(),
        "malformed URL must surface Err"
    );

    let good = request_with("GET", "https://a.example/?x=1&y=2", None, None);
    let params = good.request.query_params().unwrap();
    assert_eq!(
        params,
        vec![
            ("x".to_string(), "1".to_string()),
            ("y".to_string(), "2".to_string())
        ],
        "well-formed query string must decode both params"
    );
}
