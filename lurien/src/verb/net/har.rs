//! The session's traffic as a HAR, redacted the same way `net` is.
//!
//! A HAR is what every other tool in this space reads, so an export is how
//! evidence for a run leaves the session. It carries what the browser told us:
//! request and response headers, timings, sizes, and the request body when the
//! body is one of the two shapes a secret can be found in. Response bodies are
//! not captured at all, so the export never claims to have one.
//!
//! Redaction is the module's, not this file's: the same header allowlist, the
//! same query keys, and the same rule for cookies. A HAR that leaked what `net`
//! hides would be a way around the redaction rather than a second view of it.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{
    ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec,
};
use runtime_foxdriver::NetworkEntry;
use serde_json::{json, Value};
use std::sync::Arc;

/// HAR version this writes. 1.2 is what readers expect.
const HAR_VERSION: &str = "1.2";

/// What a body is replaced with when it carries a credential.
const REDACTED: &str = "<redacted>";

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "har",
    aliases: &["net.har"],
    domain: Domain::Net,
    summary: "Export captured traffic as a HAR 1.2 log, with credentials redacted. Writes a file when given a path.",
    args: &[
        ArgSpec {
            name: "path",
            ty: ArgType::Path,
            required: false,
            default: None,
            help: "File to write the HAR to. Without one the log is returned.",
        },
        ArgSpec {
            name: "limit",
            ty: ArgType::Int,
            required: false,
            default: Some("500"),
            help: "Matching entries to export, newest last.",
        },
        ArgSpec { name: "scan_limit", ty: ArgType::Int, required: false, default: Some("5000"), help: "Recent entries to inspect before filters. Capped at 20000." },
        ArgSpec { name: "url_pattern", ty: ArgType::Str, required: false, default: None, help: "URL substrings separated by |; any matching term is included." },
        ArgSpec { name: "methods", ty: ArgType::StrList, required: false, default: None, help: "HTTP methods to include." },
        ArgSpec { name: "statuses", ty: ArgType::StrList, required: false, default: None, help: "HTTP response statuses to include." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let limit = args.u64("limit", 500).clamp(1, 5_000) as usize;
    let scan_limit = args.u64("scan_limit", 5000).clamp(limit as u64, 20_000) as usize;
    let filter = super::EntryFilter::from_args(args)?;
    let telemetry = session.telemetry().await?;
    let entries =
        super::filtered_entries(telemetry.network.last_n(scan_limit).await, &filter, limit);
    let log = har(&entries);
    let count = entries.len();
    match args.opt_path("path") {
        Some(path) => {
            let text = serde_json::to_string_pretty(&log)
                .map_err(|e| Error::Other(format!("writing the HAR: {e}")))?;
            std::fs::write(&path, text.as_bytes()).map_err(|e| {
                Error::Other(format!(
                    "writing the HAR to {}: {e}. Check that the directory exists and is writable",
                    path.display()
                ))
            })?;
            Ok(Output::Json(json!({
                "path": path.to_string_lossy(),
                "entries": count,
                "bytes": text.len(),
            })))
        }
        None => Ok(Output::Json(json!({ "entries": count, "log": log["log"] }))),
    }
}

/// The whole HAR document for these entries.
fn har(entries: &[Arc<NetworkEntry>]) -> Value {
    json!({
        "log": {
            "version": HAR_VERSION,
            "creator": {
                "name": "lurien",
                "version": crate::version::crate_version(),
            },
            "pages": [],
            "entries": entries.iter().map(har_entry).collect::<Vec<Value>>(),
        }
    })
}

/// One HAR entry. Unknown numbers are `-1`, which is what a HAR reader expects
/// for a phase the browser did not report; a zero would read as instant.
fn har_entry(entry: &Arc<NetworkEntry>) -> Value {
    let timing = &entry.request.timing;
    let phase = |from: Option<f64>, to: Option<f64>| match (from, to) {
        (Some(from), Some(to)) if to >= from => to - from,
        _ => -1.0,
    };
    let dns = phase(timing.dns_start_ms, timing.dns_end_ms);
    let connect = phase(timing.connect_start_ms, timing.connect_end_ms);
    let ssl = phase(timing.tls_start_ms, timing.connect_end_ms);
    let wait = phase(timing.connect_end_ms, timing.response_start_ms);
    let receive = phase(timing.response_start_ms, timing.response_end_ms);
    let total: f64 = [dns, connect, wait, receive]
        .iter()
        .filter(|value| **value >= 0.0)
        .sum();

    let url = super::safe_url(&entry.request.url);
    let mut request = json!({
        "method": entry.request.method.as_str(),
        "url": url,
        "httpVersion": entry.response.as_ref().map_or("", |r| r.protocol.as_str()),
        "cookies": cookies(&entry.request.cookies),
        "headers": headers(&entry.request.headers),
        "queryString": query_string(&url),
        "headersSize": -1,
        "bodySize": entry.request.post_data.as_ref().map_or(0, String::len),
    });
    if let Some(body) = entry.request.post_data.as_ref() {
        request["postData"] = post_data(body, &entry.request.headers);
    }

    let response = match entry.response.as_ref() {
        Some(response) => json!({
            "status": response.status,
            "statusText": response.status_text,
            "httpVersion": response.protocol,
            "cookies": [],
            "headers": headers(&response.headers),
            "content": {
                "size": response.body_size.unwrap_or(0),
                "mimeType": response.mime_type,
                // Response bodies are not captured, so the export says so
                // rather than reporting an empty body as the real one.
                "comment": "body not captured",
            },
            "redirectURL": redirect_url(&response.headers),
            "headersSize": -1,
            "bodySize": response.body_size.unwrap_or(0),
            "_fromCache": response.from_cache,
        }),
        // A request that failed has no response. HAR readers want the shape, so
        // status 0 with the browser's own error text is the honest row.
        None => json!({
            "status": 0,
            "statusText": entry.error.as_ref().map_or("", |e| e.error_text.as_str()),
            "httpVersion": "",
            "cookies": [],
            "headers": [],
            "content": { "size": 0, "mimeType": "" },
            "redirectURL": "",
            "headersSize": -1,
            "bodySize": 0,
        }),
    };

    json!({
        "_ref": format!("browser_request:{}", entry.request.id),
        "startedDateTime": crate::clock::format_time(i64::try_from(entry.request.timestamp).unwrap_or_default()),
        "time": total,
        "request": request,
        "response": response,
        "cache": {},
        "timings": {
            "blocked": -1,
            "dns": dns,
            "connect": connect,
            "ssl": ssl,
            "send": 0,
            "wait": wait,
            "receive": receive,
        },
    })
}

fn headers(captured: &[runtime_foxdriver::CapturedHeader]) -> Vec<Value> {
    captured
        .iter()
        .map(|header| {
            json!({
                "name": header.name,
                "value": super::safe_header_value(&header.name, &header.value),
            })
        })
        .collect()
}

/// A cookie is a credential, so a HAR carries the names and never the values.
fn cookies(captured: &[runtime_foxdriver::network::CapturedCookie]) -> Vec<Value> {
    captured
        .iter()
        .map(|cookie| {
            json!({
                "name": cookie.name,
                "value": REDACTED,
                "domain": cookie.domain,
                "path": cookie.path,
                "httpOnly": cookie.http_only,
                "secure": cookie.secure,
            })
        })
        .collect()
}

/// The query as pairs, read back off the already redacted URL so there is one
/// rule for what a query may show.
fn query_string(url: &str) -> Vec<Value> {
    let Some((_, query)) = url.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (name, value) = part.split_once('=').unwrap_or((part, ""));
            json!({ "name": name, "value": value })
        })
        .collect()
}

/// The request body, with credentials taken out of the two shapes they are found
/// in. Anything else keeps its size and its type and loses its text: a body this
/// module cannot read is a body it cannot redact.
fn post_data(body: &str, request_headers: &[runtime_foxdriver::CapturedHeader]) -> Value {
    let mime = request_headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map_or(String::new(), |header| header.value.to_ascii_lowercase());
    if mime.contains("application/x-www-form-urlencoded") {
        let params: Vec<Value> = body
            .split('&')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let (name, value) = part.split_once('=').unwrap_or((part, ""));
                let value = if super::sensitive_query_key(name) {
                    REDACTED.to_string()
                } else {
                    value.to_string()
                };
                json!({ "name": name, "value": value })
            })
            .collect();
        let text = params
            .iter()
            .map(|pair| {
                format!(
                    "{}={}",
                    pair["name"].as_str().unwrap_or_default(),
                    pair["value"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        return json!({ "mimeType": mime, "params": params, "text": text });
    }
    if mime.contains("json") {
        if let Ok(mut value) = serde_json::from_str::<Value>(body) {
            redact_json(&mut value);
            return json!({
                "mimeType": mime,
                "params": [],
                "text": value.to_string(),
            });
        }
    }
    json!({
        "mimeType": mime,
        "params": [],
        "comment": format!("body of {} bytes omitted: only form and json bodies can be redacted", body.len()),
    })
}

/// Replace every value whose key names a credential, at any depth.
fn redact_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if super::sensitive_query_key(key) {
                    *child = Value::String(REDACTED.to_string());
                } else {
                    redact_json(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                redact_json(item);
            }
        }
        _ => {}
    }
}

/// Where a redirect points, which is the one response header a HAR reader wants
/// out of the header list.
fn redirect_url(response_headers: &[runtime_foxdriver::CapturedHeader]) -> String {
    response_headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("location"))
        .map_or(String::new(), |header| super::safe_url(&header.value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_foxdriver::network::{
        CapturedCookie, CapturedError, CapturedHeader, CapturedRequest, CapturedResponse,
        CapturedTiming,
    };

    fn header(name: &str, value: &str) -> CapturedHeader {
        CapturedHeader {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    fn timing() -> CapturedTiming {
        CapturedTiming {
            dns_start_ms: Some(10.0),
            dns_end_ms: Some(15.0),
            connect_start_ms: Some(15.0),
            connect_end_ms: Some(40.0),
            tls_start_ms: Some(25.0),
            response_start_ms: Some(90.0),
            response_end_ms: Some(120.0),
        }
    }

    fn entry(url: &str, body: Option<&str>, mime: &str) -> Arc<NetworkEntry> {
        let mut headers = vec![
            header("Authorization", "Bearer sk-live-secret"),
            header("Cookie", "sid=deadbeef"),
            header("Accept", "text/html"),
        ];
        if !mime.is_empty() {
            headers.push(header("Content-Type", mime));
        }
        Arc::new(NetworkEntry {
            request: CapturedRequest {
                id: "42".to_string(),
                context: None,
                method: "POST".to_string(),
                url: url.to_string(),
                headers,
                post_data: body.map(str::to_string),
                timestamp: 1_600_000_000_000,
                destination: "document".to_string(),
                initiator_type: None,
                timing: timing(),
                cookies: vec![CapturedCookie {
                    name: "sid".to_string(),
                    value: "deadbeef".to_string(),
                    domain: "x.test".to_string(),
                    path: "/".to_string(),
                    size: 12,
                    http_only: true,
                    secure: true,
                    same_site: "lax".to_string(),
                }],
            },
            response: Some(CapturedResponse {
                id: "42".to_string(),
                url: url.to_string(),
                protocol: "h2".to_string(),
                status: 302,
                status_text: "Found".to_string(),
                headers: vec![
                    header("Set-Cookie", "sid=deadbeef; Path=/"),
                    header("Location", "https://x.test/next?token=leaked"),
                ],
                mime_type: "text/html".to_string(),
                body_size: Some(1234),
                from_cache: false,
            }),
            error: None,
        })
    }

    /// The one claim that matters: nothing a caller could authenticate with
    /// survives the export, wherever it was hiding.
    #[test]
    fn no_credential_reaches_the_export() {
        let entries = vec![
            entry(
                "https://x.test/login?access_token=leaked&next=/home",
                Some("user=ana&password=hunter2&csrf=abc"),
                "application/x-www-form-urlencoded",
            ),
            entry(
                "https://x.test/api",
                Some(
                    "{\"user\":\"ana\",\"api_key\":\"sk-live-2\",\"nested\":{\"session\":\"s3\"}}",
                ),
                "application/json",
            ),
        ];
        let text = har(&entries).to_string();
        for secret in [
            "sk-live-secret",
            "deadbeef",
            "hunter2",
            "sk-live-2",
            "s3\"",
            "leaked",
        ] {
            assert!(
                !text.contains(secret),
                "the export carries {secret:?}: {text}"
            );
        }
        // Redaction is not deletion: the shape a reader needs is still there.
        assert!(text.contains("Bearer ***redacted***"), "{text}");
        assert!(text.contains("\"name\":\"password\""), "{text}");
        assert!(text.contains("\"name\":\"sid\""), "{text}");
        // The json body travels as text, so its own quotes are escaped.
        assert!(text.contains("\\\"user\\\":\\\"ana\\\""), "{text}");
    }

    #[test]
    fn a_har_carries_the_shape_a_reader_expects() {
        let log = har(&[entry("https://x.test/a", None, "")]);
        assert_eq!(log["log"]["version"], HAR_VERSION);
        assert_eq!(log["log"]["creator"]["name"], "lurien");
        let entry = &log["log"]["entries"][0];
        assert_eq!(entry["startedDateTime"], "2020-09-13T12:26:40.000Z");
        assert_eq!(entry["request"]["method"], "POST");
        assert_eq!(entry["response"]["status"], 302);
        assert_eq!(entry["response"]["httpVersion"], "h2");
        assert_eq!(
            entry["response"]["redirectURL"],
            "https://x.test/next?token=<redacted>"
        );
        // Timings are differences of what the browser reported, and the total is
        // the phases that were reported rather than a guess.
        assert_eq!(entry["timings"]["dns"], 5.0);
        assert_eq!(entry["timings"]["connect"], 25.0);
        assert_eq!(entry["timings"]["ssl"], 15.0);
        assert_eq!(entry["timings"]["wait"], 50.0);
        assert_eq!(entry["timings"]["receive"], 30.0);
        assert_eq!(entry["time"], 110.0);
    }

    #[test]
    fn a_phase_the_browser_did_not_report_is_minus_one() {
        let mut bare = entry("https://x.test/a", None, "");
        Arc::get_mut(&mut bare).unwrap().request.timing = CapturedTiming {
            dns_start_ms: None,
            dns_end_ms: None,
            connect_start_ms: None,
            connect_end_ms: None,
            tls_start_ms: None,
            response_start_ms: None,
            response_end_ms: None,
        };
        let log = har(&[bare]);
        let timings = &log["log"]["entries"][0]["timings"];
        for phase in ["dns", "connect", "ssl", "wait", "receive"] {
            assert_eq!(timings[phase], -1.0, "{phase} should read as unknown");
        }
        assert_eq!(log["log"]["entries"][0]["time"], 0.0);
    }

    #[test]
    fn a_body_that_cannot_be_redacted_is_not_exported() {
        let log = har(&[entry(
            "https://x.test/upload",
            Some("--boundary\r\npassword=hunter2\r\n--boundary--"),
            "multipart/form-data; boundary=boundary",
        )]);
        let post = &log["log"]["entries"][0]["request"]["postData"];
        assert!(post.get("text").is_none(), "{post}");
        assert!(
            post["comment"]
                .as_str()
                .unwrap_or_default()
                .contains("omitted"),
            "{post}"
        );
        assert!(!log.to_string().contains("hunter2"));
    }

    #[test]
    fn a_request_that_failed_is_a_row_with_the_browsers_own_words() {
        let mut failed = entry("https://x.test/gone", None, "");
        let inner = Arc::get_mut(&mut failed).unwrap();
        inner.response = None;
        inner.error = Some(CapturedError {
            id: "42".to_string(),
            url: "https://x.test/gone".to_string(),
            error_text: "NS_ERROR_ABORT".to_string(),
        });
        let log = har(&[failed]);
        let response = &log["log"]["entries"][0]["response"];
        assert_eq!(response["status"], 0);
        assert_eq!(response["statusText"], "NS_ERROR_ABORT");
    }
}
