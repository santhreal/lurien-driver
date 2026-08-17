//! Document lifecycle. Navigate, read, capture, wait.

mod back;
mod forward;
mod goto;
mod reload;
mod screenshot;
mod snapshot;
mod stop;
mod title;
mod url;
mod wait;

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Output, VerbSpec};

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &back::SPEC,
    &forward::SPEC,
    &goto::SPEC,
    &reload::SPEC,
    &screenshot::SPEC,
    &snapshot::SPEC,
    &stop::SPEC,
    &title::SPEC,
    &url::SPEC,
    &wait::SPEC,
];

/// Shared history move. BiDi has no history command, so this drives the
/// document's own history object and reports the URL it landed on.
async fn history_step(session: &Session, delta: i32) -> Result<Output, Error> {
    let browser = session.browser().await?;
    browser
        .page()
        .evaluate(format!("history.go({delta})"))
        .await
        .map_err(|e| Error::Other(format!("history.go({delta}): {e}")))?;
    // The navigation is asynchronous; settle before reading the URL back.
    browser.wait(250).await?;
    Ok(Output::Text(browser.url().await?))
}
