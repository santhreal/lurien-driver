//! Read visible text of a selector.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "text",
    aliases: &["dom.text"],
    domain: Domain::Dom,
    summary: "Visible text of the first match.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector." },
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
    let browser = session.browser().await?;
    let js = format!(
        "(() => {{ const el = document.querySelector({sel}); return el ? (el.innerText || el.textContent || '') : ''; }})()",
        sel = serde_json::Value::String(selector.to_string())
    );
    let text = browser
        .page()
        .evaluate(js)
        .await
        .map_err(|e| Error::Other(format!("text {selector}: {e}")))?
        .into_value::<String>()
        .map_err(|e| Error::Other(format!("text {selector}: {e}")))?;
    Ok(Output::Text(text))
}