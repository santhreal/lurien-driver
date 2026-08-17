//! Every cookie, HttpOnly included.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "cookies",
    aliases: &["storage.cookies"],
    domain: Domain::Storage,
    summary: "List all cookies including HttpOnly.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let cookies = session.browser().await?.cookies().await?;
    serde_json::to_value(&cookies)
        .map(Output::Json)
        .map_err(|e| Error::Other(format!("cookies: {e}")))
}