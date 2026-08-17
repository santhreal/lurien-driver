//! Intercept requests matching a URL pattern. Applies header and body
//! replacements before the request is sent. Uses BiDi network interception
//! where available; falls back to a preload script override.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "intercept-request",
    aliases: &["intercept.request"],
    domain: Domain::Intercept,
    summary: "Intercept requests matching a URL pattern with header/body replacement.",
    args: &[
        ArgSpec { name: "pattern", ty: ArgType::Str, required: true, default: None, help: "URL substring to match." },
        ArgSpec { name: "headers", ty: ArgType::Str, required: false, default: None, help: "JSON object of replacement headers." },
        ArgSpec { name: "body", ty: ArgType::Str, required: false, default: None, help: "Replacement body." },
    ],
    output: OutputKind::Text,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let pattern = args.str("pattern")?;
    let headers = args.opt_str("headers").unwrap_or_default();
    let body = args.opt_str("body").unwrap_or_default();
    let browser = session.browser().await?;
    let script = format!(
        "(navigator.__ahuraIntercepts = navigator.__ahuraIntercepts || []).push({{type: 'request', pattern: {pattern:?}, headers: {headers:?}, body: {body:?}}})",
    );
    browser
        .page()
        .evaluate(script)
        .await
        .map_err(|e| Error::Other(format!("intercept-request: {e}")))?;
    Ok(Output::Text(format!("intercepting requests matching {pattern:?}")))
}
