//! Recent requests and responses, redacted.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{
    ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec,
};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "net",
    aliases: &["net.log"],
    domain: Domain::Net,
    summary: "Recent network requests with status and redacted headers.",
    args: &[
        ArgSpec {
            name: "limit",
            ty: ArgType::Int,
            required: false,
            default: Some("25"),
            help: "Matching rows to return, newest last. Capped at 200.",
        },
        ArgSpec {
            name: "scan_limit",
            ty: ArgType::Int,
            required: false,
            default: Some("2000"),
            help: "Recent rows to inspect before filters. Capped at 10000.",
        },
        ArgSpec {
            name: "url_pattern",
            ty: ArgType::Str,
            required: false,
            default: None,
            help: "URL substrings separated by |; any matching term is included.",
        },
        ArgSpec {
            name: "methods",
            ty: ArgType::StrList,
            required: false,
            default: None,
            help: "HTTP methods to include.",
        },
        ArgSpec {
            name: "statuses",
            ty: ArgType::StrList,
            required: false,
            default: None,
            help: "HTTP response statuses to include.",
        },
        ArgSpec {
            name: "headers",
            ty: ArgType::Bool,
            required: false,
            default: None,
            help: "Include redacted request and response headers.",
        },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let limit = args.u64("limit", 25).clamp(1, 200) as usize;
    let scan_limit = args.u64("scan_limit", 2000).clamp(limit as u64, 10_000) as usize;
    let headers = args.bool("headers", false);
    let filter = super::EntryFilter::from_args(args)?;
    let telemetry = session.telemetry().await?;
    let entries =
        super::filtered_entries(telemetry.network.last_n(scan_limit).await, &filter, limit);
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
