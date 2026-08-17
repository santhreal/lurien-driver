//! Document title.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "title",
    aliases: &["page.title"],
    domain: Domain::Page,
    summary: "Document title of the active browsing context.",
    args: &[],
    output: OutputKind::Text,
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
        .title()
        .await
        .map(Output::Text)
        .map_err(|e| Error::Other(format!("title: {e}")))
}