//! Wheel scroll at the pointer.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "scroll",
    aliases: &["input.scroll"],
    domain: Domain::Input,
    summary: "Wheel scroll by dx, dy.",
    args: &[
        ArgSpec { name: "dx", ty: ArgType::Int, required: false, default: Some("0"), help: "Horizontal delta in pixels." },
        ArgSpec { name: "dy", ty: ArgType::Int, required: false, default: Some("0"), help: "Vertical delta in pixels." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let dx = args.i64("dx", 0);
    let dy = args.i64("dy", 0);
    session.browser().await?.scroll(dx, dy).await?;
    Ok(Output::Text(format!("scrolled {dx},{dy}")))
}