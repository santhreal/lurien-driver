//! Network capture. Reads the log the session armed at launch, so a request that
//! happened before the verb ran is still visible.
//!
//! Secrets never leave this module intact: header values on an auth allowlist and
//! sensitive query parameters are redacted before any face sees a row.

mod clear;
mod har;
mod log;
mod tokens;

use crate::verb::VerbSpec;
use runtime_foxdriver::{CapturedHeader, NetworkEntry};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) struct EntryFilter {
    url_terms: Vec<String>,
    methods: std::collections::BTreeSet<String>,
    statuses: std::collections::BTreeSet<u16>,
}

impl EntryFilter {
    pub(crate) fn from_args(args: &crate::verb::Args) -> Result<Self, crate::error::Error> {
        let url_terms = args
            .opt_str("url_pattern")
            .unwrap_or("")
            .split('|')
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(str::to_string)
            .collect();
        let methods = args
            .opt_str_list("methods")?
            .into_iter()
            .map(|method| method.trim().to_ascii_uppercase())
            .filter(|method| !method.is_empty())
            .collect();
        let statuses = args
            .opt_str_list("statuses")?
            .into_iter()
            .map(|status| {
                status.trim().parse::<u16>().map_err(|_| {
                    crate::error::Error::Other(format!(
                        "statuses entries must be HTTP status integers, got {status:?}"
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            url_terms,
            methods,
            statuses,
        })
    }

    pub(crate) fn matches(&self, entry: &Arc<NetworkEntry>) -> bool {
        (self.url_terms.is_empty()
            || self
                .url_terms
                .iter()
                .any(|term| entry.request.url.contains(term)))
            && (self.methods.is_empty()
                || self
                    .methods
                    .contains(&entry.request.method.as_str().to_ascii_uppercase()))
            && (self.statuses.is_empty()
                || entry
                    .status()
                    .is_some_and(|status| self.statuses.contains(&status)))
    }
}

pub(crate) fn filtered_entries(
    entries: Vec<Arc<NetworkEntry>>,
    filter: &EntryFilter,
    limit: usize,
) -> Vec<Arc<NetworkEntry>> {
    let mut matches = entries
        .into_iter()
        .filter(|entry| filter.matches(entry))
        .collect::<Vec<_>>();
    if matches.len() > limit {
        matches.drain(..matches.len() - limit);
    }
    matches
}

/// Verbs of this domain. A new verb is one line here plus its own file.
/// Registry entries for the network domain.
pub static SPECS: &[&VerbSpec] = &[&clear::SPEC, &har::SPEC, &log::SPEC, &tokens::SPEC];

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
/// keeps its scheme so a caller can still tell Bearer from Basic. A header whose
/// value is a URL goes through the same query rules as a request URL, because
/// `Location`, `Referer` and `Refresh` carry one-time tokens as often as a
/// request line does.
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
    if value.contains('?') && (value.starts_with("http://") || value.starts_with("https://")) {
        return safe_url(value);
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
        assert_eq!(
            safe_header_value("cookie", "sid=deadbeef"),
            "***redacted***"
        );
        assert_eq!(
            safe_header_value("Set-Cookie", "cf_clearance=x"),
            "***redacted***"
        );
    }

    #[test]
    fn ordinary_headers_survive() {
        assert_eq!(safe_header_value("accept", "text/html"), "text/html");
    }

    /// A URL in a header is a URL. `Location` after an OAuth hop is the usual
    /// way a one-time code escapes a redacted row.
    #[test]
    fn a_url_in_a_header_gets_the_query_rules() {
        for name in ["Location", "Referer", "Content-Location"] {
            let out = safe_header_value(name, "https://x.test/cb?code=abc&next=/home");
            assert_eq!(
                out, "https://x.test/cb?code=<redacted>&next=/home",
                "{name}"
            );
        }
        // Not a URL, not touched: a header that happens to contain a question
        // mark is still its own value.
        assert_eq!(
            safe_header_value("accept", "text/html;q=0.9?x"),
            "text/html;q=0.9?x"
        );
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

    /// Historical queries must combine URL alternatives, methods, and statuses before limiting output.
    #[test]
    fn network_filter_parses_all_dimensions() {
        let args = crate::verb::Args::from_value(serde_json::json!({
            "url_pattern": "user_voice|recommendedscreening",
            "methods": ["post", "PUT"],
            "statuses": ["200", "302"],
        }))
        .unwrap();
        let filter = EntryFilter::from_args(&args).unwrap();
        assert_eq!(filter.url_terms, vec!["user_voice", "recommendedscreening"]);
        assert!(filter.methods.contains("POST"));
        assert!(filter.methods.contains("PUT"));
        assert!(filter.statuses.contains(&200));
        assert!(filter.statuses.contains(&302));
    }

    #[test]
    fn network_filter_rejects_non_numeric_statuses() {
        let args = crate::verb::Args::from_value(serde_json::json!({
            "statuses": ["success"],
        }))
        .unwrap();
        assert!(EntryFilter::from_args(&args).is_err());
    }
}
