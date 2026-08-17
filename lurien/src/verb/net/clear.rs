//! Empty the capture buffer.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "net-clear",
    aliases: &["net.clear"],
    domain: Domain::Net,
    summary: "Empty the network log so the next read shows only new traffic.",
    args: &[],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let telemetry = session.telemetry().await?;
    telemetry.network.clear().await;
    Ok(Output::Text("network log cleared".into()))
}