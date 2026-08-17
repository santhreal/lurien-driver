//! Restore a snapshot onto the live origin.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "state-set",
    aliases: &["state.set"],
    domain: Domain::State,
    summary: "Restore a state snapshot: cookies first, then local and session storage.",
    args: &[
        ArgSpec { name: "snapshot", ty: ArgType::Str, required: true, default: None, help: "JSON snapshot produced by the state verb." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let raw = args.str("snapshot")?;
    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|e| Error::BadArgs {
        verb: "state-set".into(),
        detail: format!("snapshot was not JSON: {e}"),
    })?;
    let version = parsed.get("version").and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(super::SNAPSHOT_VERSION)) {
        return Err(Error::BadArgs {
            verb: "state-set".into(),
            detail: format!(
                "snapshot version {version:?} is not {}; re-capture it with the state verb",
                super::SNAPSHOT_VERSION
            ),
        });
    }
    let browser = session.browser().await?;
    let mut cookies_set = 0usize;
    if let Some(cookies) = parsed.get("cookies").and_then(serde_json::Value::as_array) {
        for cookie in cookies {
            let (Some(name), Some(value), Some(domain)) = (
                cookie.get("name").and_then(serde_json::Value::as_str),
                cookie.get("value").and_then(serde_json::Value::as_str),
                cookie.get("domain").and_then(serde_json::Value::as_str),
            ) else {
                continue;
            };
            browser
                .page()
                .set_cookie(
                    name,
                    value,
                    domain,
                    cookie.get("path").and_then(serde_json::Value::as_str),
                    cookie.get("expires").and_then(serde_json::Value::as_u64),
                    cookie.get("secure").and_then(serde_json::Value::as_bool),
                    cookie.get("http_only").and_then(serde_json::Value::as_bool),
                    None,
                )
                .await
                .map_err(|e| Error::Other(format!("state-set: cookie {name}: {e}")))?;
            cookies_set += 1;
        }
    }
    let storage = parsed
        .get("storage")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let js = format!(
        "(() => {{ const s = {storage}; const load = (target, kv) => {{ \
         for (const k of Object.keys(kv || {{}})) {{ try {{ target.setItem(k, kv[k]); }} catch (e) {{}} }} \
         return Object.keys(kv || {{}}).length; }}; \
         return load(localStorage, s.local) + load(sessionStorage, s.session); }})()"
    );
    let keys = browser
        .page()
        .evaluate(js)
        .await
        .map_err(|e| Error::Other(format!("state-set: write storage: {e}")))?
        .into_value::<u64>()
        .unwrap_or(0);
    Ok(Output::Json(serde_json::json!({
        "cookies": cookies_set,
        "storage_keys": keys,
    })))
}