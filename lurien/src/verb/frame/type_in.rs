//! Type inside a named frame.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "type-in",
    aliases: &["frame.type"],
    domain: Domain::Frame,
    summary: "Focus a selector inside a named frame and type into it.",
    args: &[
        ArgSpec { name: "frame", ty: ArgType::Str, required: true, default: None, help: "Frame target: a frames handle like f2, an id, index:<n>, url:<substr>, name:<name>, or main." },
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector inside that frame." },
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
    let frame = args.str("frame")?;
    let selector = args.str("selector")?;
    let text = args.str("text")?;
    let target = session.frame_target(SPEC.name, frame).await?;
    session
        .browser()
        .await?
        .page()
        .type_in_frame(&target, selector, text)
        .await
        .map_err(|e| Error::Other(format!("type into {selector} in {frame}: {e}")))?;
    Ok(Output::Text(format!("typed into {selector} in {frame}")))
}