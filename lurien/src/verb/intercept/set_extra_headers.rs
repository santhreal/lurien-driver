//! Set multiple extra request headers from a JSON object.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "set-extra-headers",
    aliases: &["intercept.set_extra_headers"],
    domain: Domain::Intercept,
    summary: "Set multiple extra request headers from a JSON object string.",
    args: &[ArgSpec {
        name: "headers",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "JSON object of header name to value.",
    }],
    output: OutputKind::Text,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let headers_json = args.str("headers")?;
    let headers: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&headers_json)
            .map_err(|e| Error::Other(format!("set-extra-headers: invalid JSON: {e}")))?;
    let browser = session.browser().await?;
    let mut script = String::from("(navigator.__ahuraHeaders = navigator.__ahuraHeaders || new Map())");
    for (k, v) in &headers {
        let val = v.as_str().unwrap_or("");
        script.push_str(&format!(".set({k:?}, {val:?})"));
    }
    browser
        .page()
        .evaluate(script)
        .await
        .map_err(|e| Error::Other(format!("set-extra-headers: {e}")))?;
    Ok(Output::Text(format!("set {} extra headers", headers.len())))
}
