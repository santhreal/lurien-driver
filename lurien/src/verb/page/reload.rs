//! Reload the active context.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "reload",
    aliases: &["page.reload"],
    domain: Domain::Page,
    summary: "Reload the active document.",
    args: &[],
    output: OutputKind::Empty,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let browser = session.browser().await?;
    browser
        .page()
        .reload()
        .await
        .map(|()| Output::Empty)
        .map_err(|e| Error::Other(format!("reload: {e}")))
}