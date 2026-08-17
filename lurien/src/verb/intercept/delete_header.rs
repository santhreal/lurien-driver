//! Delete a request header override.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "delete-header",
    aliases: &["intercept.delete_header"],
    domain: Domain::Intercept,
    summary: "Delete a request header override.",
    args: &[ArgSpec {
        name: "name",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "Header name to remove.",
    }],
    output: OutputKind::Text,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let name = args.str("name")?;
    let browser = session.browser().await?;
    browser
        .page()
        .evaluate(format!(
            "(navigator.__ahuraHeaders || new Map()).delete({name:?})",
        ))
        .await
        .map_err(|e| Error::Other(format!("delete-header: {e}")))?;
    Ok(Output::Text(format!("deleted header {name}")))
}
