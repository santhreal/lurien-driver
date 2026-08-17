//! Focus a field and type into it.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "fill",
    aliases: &["dom.fill"],
    domain: Domain::Dom,
    summary: "Focus selector and type text.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector of the field." },
        ArgSpec { name: "text", ty: ArgType::Str, required: true, default: None, help: "Text to type." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let selector = args.str("selector")?;
    let text = args.str("text")?;
    session.browser().await?.fill(selector, text).await?;
    Ok(Output::Text(format!("filled {selector}")))
}