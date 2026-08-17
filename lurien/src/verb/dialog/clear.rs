//! Empty the dialog buffer.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "dialog-clear",
    aliases: &["dialog.clear"],
    domain: Domain::Dialog,
    summary: "Empty the dialog log so the next read shows only new dialogs.",
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
    telemetry.dialogs.clear().await;
    Ok(Output::Text("dialog log cleared".into()))
}