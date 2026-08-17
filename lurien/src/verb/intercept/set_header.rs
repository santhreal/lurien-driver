//! Set a request header override via eval. These headers are injected
//! on subsequent navigations through a BiDi preload script.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "set-header",
    aliases: &["intercept.set_header"],
    domain: Domain::Intercept,
    summary: "Set a request header override for subsequent navigations.",
    args: &[
        ArgSpec { name: "name", ty: ArgType::Str, required: true, default: None, help: "Header name." },
        ArgSpec { name: "value", ty: ArgType::Str, required: false, default: None, help: "Header value. Omit to clear." },
    ],
    output: OutputKind::Text,
    stability: Stability::Preview,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let name = args.str("name")?;
    let value = args.opt_str("value").unwrap_or_default();
    let browser = session.browser().await?;
    let script = format!(
        "(navigator.__ahuraHeaders = navigator.__ahuraHeaders || new Map()).set({name:?}, {value:?})",
    );
    browser
        .page()
        .evaluate(script)
        .await
        .map_err(|e| Error::Other(format!("set-header: {e}")))?;
    Ok(Output::Text(format!("set header {name}")))
}
