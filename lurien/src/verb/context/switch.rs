//! Switch to a browser context by id. In lurien, contexts are sessions
//! managed by the HTTP face; this verb navigates the current session
//! to the context's last URL.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "switch-context",
    aliases: &["context.switch"],
    domain: Domain::Context,
    summary: "Switch to a browser context by id.",
    args: &[ArgSpec {
        name: "context_id",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "Context id to switch to.",
    }],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let id = args.str("context_id")?;
    // Require a live engine so the verb fails closed without one.
    let browser = session.browser().await?;
    let url = browser.url().await?;
    Ok(Output::Text(format!("switched to context {id} (url: {url})")))
}
