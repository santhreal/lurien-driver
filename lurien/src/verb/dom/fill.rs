//! Focus a field and type into it, once the field is there and actionable.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "fill",
    aliases: &["dom.fill"],
    domain: Domain::Dom,
    summary: "Focus a field and type text, waiting for it to be actionable.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS, or role:/text:/label:/placeholder:/testid: form." },
        ArgSpec { name: "text", ty: ArgType::Str, required: true, default: None, help: "Text to type." },
        crate::verb::TIMEOUT_ARG,
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
    let timeout_ms = crate::verb::timeout_ms(args);
    session
        .browser()
        .await?
        .fill_within(selector, text, timeout_ms)
        .await?;
    Ok(Output::Text(format!("filled {selector}")))
}
