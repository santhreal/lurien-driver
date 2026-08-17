//! What time this session's pages read.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "clock",
    aliases: &["profile.clock"],
    domain: Domain::Profile,
    summary: "Report the time pages read from this session and how far it runs from the host's.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &crate::verb::Args) -> Result<Output, Error> {
    let control = session.control()?;
    session.browser().await?;
    Ok(Output::Json(report(&crate::clock::read(control).await?)))
}

/// One shape for reading and for moving, so a caller never has to reconcile two.
pub(crate) fn report(reading: &crate::clock::Reading) -> serde_json::Value {
    serde_json::json!({
        "epoch_ms": reading.epoch_ms,
        "time": crate::clock::format_time(reading.epoch_ms),
        "shift_ms": reading.shift_ms,
        "source": if reading.is_shifted() { "session" } else { "host" },
    })
}
