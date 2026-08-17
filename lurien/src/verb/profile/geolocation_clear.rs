//! Go back to the persona's own position.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "geolocation-clear",
    aliases: &["profile.geolocation-clear", "geo-clear"],
    domain: Domain::Profile,
    summary: "Drop a position override and serve the persona's own coordinates again.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let geo = session.geo()?;
    session.browser().await?;
    geo.clear().await?;
    Ok(Output::Json(super::geolocation::report(session, geo)?))
}
