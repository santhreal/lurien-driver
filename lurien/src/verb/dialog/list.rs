//! Every dialog and download seen so far.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "dialogs",
    aliases: &["dialog.list"],
    domain: Domain::Dialog,
    summary: "Dialogs captured, dialogs still open, and downloads.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let telemetry = session.telemetry().await?;
    let all = telemetry.dialogs.dialogs().await;
    let open = telemetry.dialogs.open_dialogs().await;
    let downloads = telemetry.dialogs.downloads().await;
    Ok(Output::Json(serde_json::json!({
        "count": all.len(),
        "open": open,
        "dialogs": all,
        "downloads": downloads,
    })))
}