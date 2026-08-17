//! Recent requests and responses, redacted.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "net",
    aliases: &["net.log"],
    domain: Domain::Net,
    summary: "Recent network requests with status and redacted headers.",
    args: &[
        ArgSpec { name: "limit", ty: ArgType::Int, required: false, default: Some("25"), help: "Rows to return, newest last. Capped at 200." },
        ArgSpec { name: "headers", ty: ArgType::Bool, required: false, default: None, help: "Include redacted request and response headers." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let limit = args.u64("limit", 25).min(200) as usize;
    let headers = args.bool("headers", false);
    let telemetry = session.telemetry().await?;
    let entries = telemetry.network.last_n(limit).await;
    let rows: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| super::entry_row(e, headers))
        .collect();
    let metrics = telemetry.network.metrics().await;
    Ok(Output::Json(serde_json::json!({
        "count": rows.len(),
        "entries": rows,
        "requests_received": metrics.requests_received,
        "responses_received": metrics.responses_received,
    })))
}