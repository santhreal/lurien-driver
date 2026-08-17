//! How many elements match, and how many of those are visible.

use crate::error::Error;
use crate::locator;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "count",
    aliases: &["dom.count"],
    domain: Domain::Dom,
    summary: "Number of elements matching a selector, total and visible.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS, or role:/text:/label:/placeholder:/testid: form." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let selector = args.str("selector")?;
    let browser = session.browser().await?;
    // Counting never waits: zero is an answer, and an element that has not
    // arrived yet is exactly what a caller counting is trying to find out.
    let (count, visible) = locator::count(browser.page(), selector).await?;
    Ok(Output::Json(serde_json::json!({
        "selector": selector,
        "count": count,
        "visible": visible,
    })))
}
