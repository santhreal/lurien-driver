//! How many elements match.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "count",
    aliases: &["dom.count"],
    domain: Domain::Dom,
    summary: "Number of elements matching selector.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let selector = args.str("selector")?;
    let browser = session.browser().await?;
    let found = browser
        .page()
        .find_elements(selector)
        .await
        .map_err(|e| Error::Other(format!("count {selector}: {e}")))?;
    Ok(Output::Json(serde_json::json!({
        "selector": selector,
        "count": found.len(),
    })))
}