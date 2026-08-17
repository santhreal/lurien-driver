//! List active browser contexts (sessions).

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "contexts",
    aliases: &["context.list"],
    domain: Domain::Context,
    summary: "List active browser contexts (sessions).",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, _args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session))
}

async fn run(session: &Session) -> Result<Output, Error> {
    // Require a live engine so the verb fails closed without one.
    let browser = session.browser().await?;
    let url = browser.url().await?;
    let info = serde_json::json!({
        "active": true,
        "current_url": url,
    });
    Ok(Output::Json(info))
}
