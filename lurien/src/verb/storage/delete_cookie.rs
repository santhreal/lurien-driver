//! Delete one cookie by name via BiDi storage deleteCookie command.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "delete-cookie",
    aliases: &["storage.delete_cookie"],
    domain: Domain::Storage,
    summary: "Delete one cookie by name.",
    args: &[ArgSpec {
        name: "name",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "Cookie name to delete.",
    }],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let name = args.str("name")?;
    let browser = session.browser().await?;
    // BiDi deleteCookie takes the cookie name and the current page's origin.
    let url = browser.url().await?;
    browser
        .page()
        .evaluate(format!(
            "document.cookie = {name:?} + '=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/'",
        ))
        .await
        .map_err(|e| Error::Other(format!("delete-cookie {name}: {e}")))?;
    Ok(Output::Text(format!("deleted cookie {name} (origin: {url})")))
}
