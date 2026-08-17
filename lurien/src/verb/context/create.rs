//! Create a new browser context (session). Navigates to url if given.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "new-context",
    aliases: &["context.new"],
    domain: Domain::Context,
    summary: "Create a new browser context. Navigates to url if given.",
    args: &[ArgSpec {
        name: "url",
        ty: ArgType::Str,
        required: false,
        default: None,
        help: "URL to navigate to in the new context.",
    }],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let browser = session.browser().await?;
    if let Some(url) = args.opt_str("url") {
        browser.goto(&url).await?;
        Ok(Output::Text(format!("new context at {url}")))
    } else {
        Ok(Output::Text("new context created".to_string()))
    }
}
