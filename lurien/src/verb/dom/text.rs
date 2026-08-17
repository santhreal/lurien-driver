//! Read visible text, once the element exists.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "text",
    aliases: &["dom.text"],
    domain: Domain::Dom,
    summary: "Visible text of an element, waiting for it to appear.",
    args: &[
        crate::verb::SELECTOR_ARG,
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
    let timeout_ms = crate::verb::timeout_ms(args);
    let browser = session.browser().await?;
    // A read is satisfied by an element that is present. A hidden element still
    // has text, and refusing to read it would make `text` less useful than the
    // DOM it reports on.
    let found = browser.locate_present(selector, timeout_ms).await?;
    let js = format!(
        "(() => {{ const el = document.querySelector({sel}); return el ? (el.innerText || el.textContent || '') : ''; }})()",
        sel = serde_json::Value::String(found.css.clone())
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
