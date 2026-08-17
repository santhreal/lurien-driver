//! Title, URL, and a text snapshot of the document.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "snapshot",
    aliases: &["page.snapshot"],
    domain: Domain::Page,
    summary: "Title, URL, and a short text snapshot of the page.",
    args: &[],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    Ok(Output::Text(session.browser().await?.snapshot().await?))
}