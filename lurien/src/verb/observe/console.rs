//! Console output and uncaught errors.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "console",
    aliases: &["observe.console"],
    domain: Domain::Observe,
    summary: "Console entries and uncaught errors captured by the sensor grid.",
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
    if !telemetry.sensors {
        return Err(Error::Other(
            "sensor grid is not installed (LURIEN_SENSORS=0); console was not captured".into(),
        ));
    }
    let browser = session.browser().await?;
    let signals = browser
        .page()
        .read_signals(false)
        .await
        .map_err(|e| Error::Other(format!("console: {e}")))?;
    let console = signals
        .get("console")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let errors = signals
        .get("errors")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    Ok(Output::Json(serde_json::json!({
        "count": console.as_array().map_or(0, Vec::len),
        "uncaught_errors": errors.as_array().map_or(0, Vec::len),
        "console": console,
        "errors": errors,
    })))
}