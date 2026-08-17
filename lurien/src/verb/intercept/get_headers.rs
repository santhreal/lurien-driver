//! Get request headers via BiDi network interception.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "get-headers",
    aliases: &["intercept.get_headers"],
    domain: Domain::Intercept,
    summary: "Get the request headers that would be sent on the next navigation.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, _args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session))
}

async fn run(session: &Session) -> Result<Output, Error> {
    let browser = session.browser().await?;
    let result = browser
        .page()
        .evaluate(
            "JSON.stringify(Object.fromEntries(navigator.__ahuraHeaders || new Map()))",
        )
        .await
        .map_err(|e| Error::Other(format!("get-headers: {e}")))?;
    let headers: serde_json::Value = result.into_value().unwrap_or(serde_json::json!({}));
    Ok(Output::Json(headers))
}
