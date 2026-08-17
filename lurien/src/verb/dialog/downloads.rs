//! What this session has downloaded, and where the files are.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "downloads",
    aliases: &["dialog.downloads"],
    domain: Domain::Dialog,
    summary: "Files this session downloaded, with status, local path and size.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let rows = crate::download::list(session).await?;
    Ok(Output::Json(serde_json::json!({
        "count": rows.len(),
        "dir": crate::download::dir_of(session).to_string_lossy(),
        "downloads": rows,
    })))
}
