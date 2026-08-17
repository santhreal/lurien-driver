//! Intercept responses matching a URL pattern. Applies header and body
//! replacements before the response is delivered to the page.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "intercept-response",
    aliases: &["intercept.response"],
    domain: Domain::Intercept,
    summary: "Intercept responses matching a URL pattern with header/body replacement.",
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
        "(navigator.__ahuraIntercepts = navigator.__ahuraIntercepts || []).push({{type: 'response', pattern: {pattern:?}, headers: {headers:?}, body: {body:?}}})",
    );
    browser
        .page()
        .evaluate(script)
        .await
        .map_err(|e| Error::Other(format!("intercept-response: {e}")))?;
    Ok(Output::Text(format!("intercepting responses matching {pattern:?}")))
}
