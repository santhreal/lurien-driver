//! Choose an option in a select element.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "select",
    aliases: &["dom.select"],
    domain: Domain::Dom,
    summary: "Select an option by value, waiting for the control to be actionable.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS, or role:/text:/label:/placeholder:/testid: form." },
        ArgSpec { name: "value", ty: ArgType::Str, required: true, default: None, help: "Option value to choose." },
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
    let value = args.str("value")?;
    let timeout_ms = crate::verb::timeout_ms(args);
    let browser = session.browser().await?;
    let found = browser.locate(selector, timeout_ms).await?;
    let js = format!(
        "(() => {{ const el = document.querySelector({sel}); if (!el) return 'missing'; \
         el.value = {val}; el.dispatchEvent(new Event('input', {{bubbles:true}})); \
         el.dispatchEvent(new Event('change', {{bubbles:true}})); return el.value; }})()",
        sel = js_string(&found.css),
        val = js_string(value),
    );
    let got = browser
        .page()
        .evaluate(js)
        .await
        .map_err(|e| Error::Other(format!("select {selector}: {e}")))?
        .into_value::<String>()
        .map_err(|e| Error::Other(format!("select {selector}: {e}")))?;
    if got == "missing" {
        return Err(Error::Other(format!("select: no element matches {selector}")));
    }
    if got != value {
        return Err(Error::Other(format!(
            "select {selector}: {value:?} is not an option (value is {got:?})"
        )));
    }
    Ok(Output::Text(format!("selected {value} in {selector}")))
}

/// JSON is a valid JavaScript string literal, so this is the escape.
fn js_string(raw: &str) -> String {
    serde_json::Value::String(raw.to_string()).to_string()
}