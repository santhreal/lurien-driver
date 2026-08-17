//! What this session does to requests before they are sent.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "route",
    aliases: &["intercept.route"],
    domain: Domain::Intercept,
    summary: "Report the route table in the order the engine tries it, with how many requests each route took.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    // The counts come from the engine that is applying the routes, so there is
    // nothing to report until it is running.
    session.browser().await?;
    Ok(Output::Json(session.route_report().await?))
}
