//! Where credentials appear in captured traffic, never what they are.
//!
//! A token inventory answers "is this request authenticated, and by what" and
//! feeds a replay decision, so it reports location, name, scheme, and length.
//! The value itself never leaves the browser through this verb.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};
use serde_json::json;
use std::collections::BTreeSet;

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "net-tokens",
    aliases: &["net.tokens"],
    domain: Domain::Net,
    summary: "Where credentials appear in captured traffic: header, query, or cookie.",
    args: &[ArgSpec {
        name: "limit",
        ty: ArgType::Int,
        required: false,
        default: Some("200"),
        help: "Requests to scan, newest last. Capped at 2000.",
    }],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

/// Request headers that carry a credential.
const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
    "proxy-authorization",
];

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let limit = args.u64("limit", 200).min(2000) as usize;
    let telemetry = session.telemetry().await?;
    let entries = telemetry.network.last_n(limit).await;

    let mut rows = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in &entries {
        let url = super::safe_url(&entry.request.url);
        for header in &entry.request.headers {
            let name = header.name.to_ascii_lowercase();
            if !CREDENTIAL_HEADERS.contains(&name.as_str()) {
                continue;
            }
            if !seen.insert(("request_header".to_string(), name.clone())) {
                continue;
            }
            rows.push(json!({
                "location": "request_header",
                "name": name,
                "scheme": scheme_of(&name, &header.value),
                "length": header.value.len(),
                "first_seen": format!("browser_request:{}", entry.request.id),
                "url": url,
            }));
        }
        if let Ok(params) = entry.request.query_params() {
            for (key, value) in params {
                let lower = key.to_ascii_lowercase();
                if !super::sensitive_query_key(&lower) {
                    continue;
                }
                if !seen.insert(("query".to_string(), lower.clone())) {
                    continue;
                }
                rows.push(json!({
                    "location": "query",
                    "name": lower,
                    "scheme": serde_json::Value::Null,
                    "length": value.len(),
                    "first_seen": format!("browser_request:{}", entry.request.id),
                    "url": url,
                }));
            }
        }
        let Some(response) = entry.response.as_ref() else {
            continue;
        };
        for header in &response.headers {
            if !header.name.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let cookie_name = header
                .value
                .split_once('=')
                .map_or(header.value.as_str(), |(n, _)| n)
                .trim()
                .to_string();
            if !seen.insert(("set_cookie".to_string(), cookie_name.clone())) {
                continue;
            }
            rows.push(json!({
                "location": "set_cookie",
                "name": cookie_name,
                "scheme": serde_json::Value::Null,
                "length": header.value.len(),
                "first_seen": format!("browser_request:{}", entry.request.id),
                "url": url,
            }));
        }
    }

    Ok(Output::Json(json!({
        "count": rows.len(),
        "scanned": entries.len(),
        "tokens": rows,
    })))
}

/// `Authorization: Bearer x` has a scheme worth reporting; nothing else does.
fn scheme_of(name: &str, value: &str) -> serde_json::Value {
    if name != "authorization" {
        return serde_json::Value::Null;
    }
    match value.split_once(' ') {
        Some((scheme, _)) => json!(scheme),
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_authorization_reports_a_scheme() {
        assert_eq!(scheme_of("authorization", "Bearer abc"), json!("Bearer"));
        assert_eq!(scheme_of("authorization", "opaque"), serde_json::Value::Null);
        assert_eq!(scheme_of("cookie", "sid=1"), serde_json::Value::Null);
    }

    #[test]
    fn the_credential_header_list_matches_what_redaction_covers() {
        // A header worth inventorying must also be a header we redact, or the
        // inventory would advertise a secret the log still prints.
        for name in CREDENTIAL_HEADERS {
            let shown = super::super::safe_header_value(name, "secret-value");
            assert!(
                !shown.contains("secret-value"),
                "{name} is inventoried but not redacted"
            );
        }
    }
}
