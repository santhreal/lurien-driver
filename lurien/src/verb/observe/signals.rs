//! Full passive-telemetry snapshot.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "signals",
    aliases: &["observe.signals"],
    domain: Domain::Observe,
    summary: "DOM-XSS sinks, console, errors, CSP violations, and postMessage traffic.",
    args: &[
        ArgSpec { name: "clear", ty: ArgType::Bool, required: false, default: None, help: "Empty the buffer so the next read returns only new signals." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let telemetry = session.telemetry().await?;
    if !telemetry.sensors {
        return Err(Error::Other(
            "sensor grid is not installed (LURIEN_SENSORS=0); no signals were captured".into(),
        ));
    }
    let browser = session.browser().await?;
    let signals = browser
        .page()
        .read_signals(args.bool("clear", false))
        .await
        .map_err(|e| Error::Other(format!("signals: {e}")))?;
    let mut out = serde_json::Map::new();
    for key in super::SIGNAL_KEYS {
        if let Some(value) = signals.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    Ok(Output::Json(serde_json::Value::Object(out)))
}