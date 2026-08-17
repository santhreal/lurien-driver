//! Trusted click inside a named frame.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "click-in",
    aliases: &["frame.click"],
    domain: Domain::Frame,
    summary: "Click a selector inside a named frame, including a cross-origin one.",
    args: &[
        ArgSpec { name: "frame", ty: ArgType::Str, required: true, default: None, help: "Frame target: id, url substring, name, or main." },
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector inside that frame." },
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
    session
        .browser()
        .await?
        .page()
        .click_in_frame(frame, selector)
        .await
        .map_err(|e| Error::Other(format!("click {selector} in {frame}: {e}")))?;
    Ok(Output::Text(format!("clicked {selector} in {frame}")))
}