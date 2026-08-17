//! Browsing contexts, main document first.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "frames",
    aliases: &["frame.list"],
    domain: Domain::Frame,
    summary: "List browsing contexts with a stable handle each: main document first, then every iframe.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    // Reading the tree is what names a frame, so a caller that has run this verb
    // can address any frame in it by a handle that will not move.
    let frames = session.frame_rows().await?;
    Ok(Output::Json(serde_json::json!({
        "count": frames.len(),
        "frames": frames,
    })))
}