//! Human-curve pointer move.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "mouse",
    aliases: &["input.mouse", "mouse_move"],
    domain: Domain::Input,
    summary: "Move the pointer to x, y along a human curve.",
    args: &[
        ArgSpec { name: "x", ty: ArgType::Float, required: true, default: None, help: "Viewport x." },
        ArgSpec { name: "y", ty: ArgType::Float, required: true, default: None, help: "Viewport y." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let x = args.f64("x", 0.0);
    let y = args.f64("y", 0.0);
    session
        .browser()
        .await?
        .page()
        .move_mouse_to(x, y)
        .await
        .map_err(|e| Error::Other(format!("mouse {x},{y}: {e}")))?;
    Ok(Output::Text(format!("moved to {x},{y}")))
}