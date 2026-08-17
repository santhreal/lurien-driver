//! Trusted key press in the active context.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "press",
    aliases: &["input.press", "key"],
    domain: Domain::Input,
    summary: "Press a key in the active context.",
    args: &[
        ArgSpec { name: "key", ty: ArgType::Str, required: true, default: None, help: "Key value, e.g. Enter, Tab, a." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let key = args.str("key")?;
    session
        .browser()
        .await?
        .page()
        .key_press(key)
        .await
        .map_err(|e| Error::Other(format!("press {key}: {e}")))?;
    Ok(Output::Text(format!("pressed {key}")))
}