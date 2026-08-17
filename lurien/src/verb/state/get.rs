//! Snapshot cookies and web storage.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "state",
    aliases: &["state.get"],
    domain: Domain::State,
    summary: "Snapshot cookies, localStorage, and sessionStorage for restore.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &Args) -> Result<Output, Error> {
    let browser = session.browser().await?;
    let cookies = browser.cookies().await?;
    let raw = browser
        .page()
        .evaluate(super::READ_STORAGE_JS)
        .await
        .map_err(|e| Error::Other(format!("state: read storage: {e}")))?
        .into_value::<String>()
        .map_err(|e| Error::Other(format!("state: decode storage: {e}")))?;
    let storage: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| Error::Other(format!("state: storage was not JSON: {e}")))?;
    Ok(Output::Json(serde_json::json!({
        "version": super::SNAPSHOT_VERSION,
        "url": browser.url().await.unwrap_or_default(),
        "cookies": cookies,
        "storage": storage,
    })))
}