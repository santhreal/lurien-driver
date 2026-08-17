//! Network capture. Reads the log the session armed at launch, so a request that
//! happened before the verb ran is still visible.
//!
//! Secrets never leave this module intact: header values on an auth allowlist and
//! sensitive query parameters are redacted before any face sees a row.

mod clear;
mod log;
mod tokens;

use crate::verb::VerbSpec;
use runtime_foxdriver::{CapturedHeader, NetworkEntry};
use serde_json::{json, Value};
use std::sync::Arc;

/// Verbs of this domain. A new verb is one line here plus its own file.
/// Registry entries for the network domain.
pub static SPECS: &[&VerbSpec] = &[&clear::SPEC, &log::SPEC, &tokens::SPEC];

/// Row a face may see: identity, outcome, and redacted headers.
pub(crate) fn entry_row(entry: &Arc<NetworkEntry>, headers: bool) -> Value {
    let mut row = json!({
        "ref": format!("browser_request:{}", entry.request.id),
        "method": entry.request.method.as_str(),
        "url": safe_url(&entry.request.url),
        "status": entry.status(),
        "destination": entry.request.destination.as_str(),
        "has_response": entry.has_response(),
        "is_error": entry.is_error(),
    });
    if headers {
        row["request_headers"] = json!(safe_headers(&entry.request.headers));
        row["response_headers"] = entry
            .response
            .as_ref()
            .map(|r| json!(safe_headers(&r.headers)))
            .unwrap_or_else(|| json!([]));
    }
    row
}

fn safe_headers(headers: &[CapturedHeader]) -> Vec<Value> {
    headers
        .iter()
        .map(|h| {
            json!({
                "name": h.name.as_str(),
                "value": safe_header_value(&h.name, &h.value),
            })
        })
        .collect()
}

/// Credential-bearing headers are replaced, not truncated. `Authorization`
/// keeps its scheme so a caller can still tell Bearer from Basic.
pub(crate) fn safe_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    const SECRET_HEADERS: &[&str] = &[
        "authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "x-auth-token",
        "x-csrf-token",
        "x-xsrf-token",
        "proxy-authorization",
    ];
    if SECRET_HEADERS.contains(&lower.as_str()) {
        if lower == "authorization" {
            if let Some((scheme, _)) = value.split_once(' ') {
                return format!("{scheme} ***redacted***");
            }
        }
        return "***redacted***".to_string();
    }
    value.to_string()
}

/// Drop the fragment and redact sensitive query values, keeping the path so the
/// row is still useful for correlation.
pub(crate) fn safe_url(raw: &str) -> String {
    let without_fragment = raw.split('#').next().unwrap_or(raw);
    let Some((base, query)) = without_fragment.split_once('?') else {
        return without_fragment.to_string();
    };
    if query.trim().is_empty() {
        return base.to_string();
    }
    let redacted = query
        .split('&')
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            if sensitive_query_key(key) {
                if value.is_empty() {
                    key.to_string()
                } else {
                    format!("{key}=<redacted>")
                }
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{redacted}")
}

pub(crate) fn sensitive_query_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "pwd",
        "otp",
        "totp",
        "mfa",
        "code",
        "csrf",
        "xsrf",
        "auth",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "id_token",
        "session",
        "sid",
        "cf_clearance",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_keeps_its_scheme_and_loses_its_secret() {
        let out = safe_header_value("Authorization", "Bearer abcdef123");
        assert_eq!(out, "Bearer ***redacted***");
        assert!(!out.contains("abcdef123"));
    }

    #[test]
    fn cookie_headers_are_replaced_whole() {
        assert_eq!(safe_header_value("cookie", "sid=deadbeef"), "***redacted***");
        assert_eq!(
            safe_header_value("Set-Cookie", "cf_clearance=x"),
            "***redacted***"
        );
    }

    #[test]
    fn ordinary_headers_survive() {
        assert_eq!(safe_header_value("accept", "text/html"), "text/html");
    }

    #[test]
    fn sensitive_query_values_are_redacted_and_the_path_survives() {
        let out = safe_url("https://x.test/cb?code=abc&next=/home#frag");
        assert_eq!(out, "https://x.test/cb?code=<redacted>&next=/home");
        assert!(!out.contains("abc"));
    }

    #[test]
    fn a_url_without_a_query_is_untouched() {
        assert_eq!(safe_url("https://x.test/a/b"), "https://x.test/a/b");
    }
}
