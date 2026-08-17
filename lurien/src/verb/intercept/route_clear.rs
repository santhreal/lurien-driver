//! Let requests go out untouched again.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "route-clear",
    aliases: &["intercept.route-clear"],
    domain: Domain::Intercept,
    summary: "Drop every route, so requests reach the network untouched again.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    // The table lives on the engine, so there is nothing to drop until it runs.
    session.browser().await?;
    let dropped = session.clear_routes().await?;
    Ok(Output::Json(serde_json::json!({
        "dropped": dropped,
        "routes": [],
        "count": 0,
    })))
}
