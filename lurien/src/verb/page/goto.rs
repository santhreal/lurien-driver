//! Navigate. Captcha is a property of this verb, not a separate tool.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "goto",
    aliases: &["page.goto", "navigate"],
    domain: Domain::Page,
    summary: "Navigate. Captcha is automatic (score-class only in v1). No challenge tool.",
    args: &[
        ArgSpec { name: "url", ty: ArgType::Str, required: true, default: None, help: "Absolute URL to open." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let url = args.str("url")?;
    let outcome = session.browser().await?.goto(url).await?;
    Ok(Output::Json(serde_json::json!({
        "url": outcome.url,
        "kind": format!("{:?}", outcome.kind).to_ascii_lowercase(),
    })))
}