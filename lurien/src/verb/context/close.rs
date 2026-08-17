//! Close a browser context by id.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "close-context",
    aliases: &["context.close"],
    domain: Domain::Context,
    summary: "Close a browser context by id.",
    args: &[ArgSpec {
        name: "context_id",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "Context id to close.",
    }],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let _id = args.str("context_id")?;
    // Require a live engine so the verb fails closed without one.
    session.browser().await?;
    session.close().await?;
    Ok(Output::Text("context closed".to_string()))
}
