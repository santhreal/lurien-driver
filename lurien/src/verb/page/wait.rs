//! Sleep. The bound is the caller's.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "wait",
    aliases: &["page.wait", "sleep"],
    domain: Domain::Page,
    summary: "Sleep ms milliseconds.",
    args: &[
        ArgSpec { name: "ms", ty: ArgType::Int, required: false, default: Some("1000"), help: "Milliseconds to sleep." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let ms = args.u64("ms", 1_000);
    // A verb that only sleeps still requires a live page: a face must not be
    // able to use `wait` as a way to look busy without an engine.
    session.browser().await?.wait(ms).await?;
    Ok(Output::Text(format!("waited {ms}ms")))
}