//! Nested browsing-context tree with depth.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "frame-tree",
    aliases: &["frame.tree"],
    domain: Domain::Frame,
    summary: "Browsing-context tree with parent and depth, including OOPIFs.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let browser = session.browser().await?;
    let nodes = browser
        .page()
        .frame_tree()
        .await
        .map_err(|e| Error::Other(format!("frame-tree: {e}")))?;
    let rows: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id.as_ref(),
                "url": n.url,
                "parent": n.parent.as_ref().map(AsRef::<str>::as_ref),
                "depth": n.depth,
            })
        })
        .collect();
    Ok(Output::Json(serde_json::json!({ "count": rows.len(), "frames": rows })))
}