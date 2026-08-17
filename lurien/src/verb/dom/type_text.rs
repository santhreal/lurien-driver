//! Type into whatever is focused.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "type",
    aliases: &["dom.type"],
    domain: Domain::Dom,
    summary: "Type text into the focused element.",
    args: &[
        ArgSpec { name: "text", ty: ArgType::Str, required: true, default: None, help: "Text to type with human keystroke timing." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let text = args.str("text")?;
    session.browser().await?.type_text(text).await?;
    Ok(Output::Text("typed".into()))
}