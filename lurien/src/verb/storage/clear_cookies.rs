//! Delete all cookies for the current page.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

pub static SPEC: VerbSpec = VerbSpec {
    name: "clear-cookies",
    aliases: &["storage.clear_cookies"],
    domain: Domain::Storage,
    summary: "Delete all cookies for the current page.",
    args: &[],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, _args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session))
}

async fn run(session: &Session) -> Result<Output, Error> {
    let browser = session.browser().await?;
    let cookies = browser.cookies().await?;
    let count = cookies.len();
    for c in &cookies {
        let _ = browser
            .page()
            .evaluate(format!(
                "document.cookie = {:?} + '=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/'",
                c.name,
            ))
            .await;
    }
    Ok(Output::Text(format!("cleared {count} cookies")))
}
