//! Stop loading the active document.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "stop",
    aliases: &["page.stop"],
    domain: Domain::Page,
    summary: "Stop loading the active document.",
    args: &[],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, _args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session))
}

async fn run(session: &Session) -> Result<Output, Error> {
    let browser = session.browser().await?;
    browser
        .page()
        .evaluate("window.stop()")
        .await
        .map_err(|e| Error::Other(format!("stop: {e}")))?;
    Ok(Output::Text("stopped".to_string()))
}
