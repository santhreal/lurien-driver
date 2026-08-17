//! Clear all request/response interception rules.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "clear-intercepts",
    aliases: &["intercept.clear"],
    domain: Domain::Intercept,
    summary: "Clear all request/response interception rules.",
    args: &[],
    output: OutputKind::Text,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, _args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session))
}

async fn run(session: &Session) -> Result<Output, Error> {
    let browser = session.browser().await?;
    browser
        .page()
        .evaluate("navigator.__ahuraIntercepts = []")
        .await
        .map_err(|e| Error::Other(format!("clear-intercepts: {e}")))?;
    Ok(Output::Text("cleared all intercepts".to_string()))
}
