//! Drop storage, workers, and caches.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "state-clear",
    aliases: &["state.clear"],
    domain: Domain::State,
    summary: "Clear web storage, unregister service workers, and delete caches.",
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
    let page = browser.page();
    // Each step is independent: a page with no service worker still gets its
    // storage cleared, and the result says which steps landed.
    let storage = page
        .evaluate(r#"(() => { try { localStorage.clear(); sessionStorage.clear(); return true; } catch (e) { return false; } })()"#)
        .await
        .ok()
        .and_then(|v| v.into_value::<bool>().ok())
        .unwrap_or(false);
    let workers = page
        .evaluate_await(
            r#"navigator.serviceWorker.getRegistrations().then(rs => Promise.all(rs.map(r => r.unregister()))).then(rs => rs.length).catch(() => 0)"#,
        )
        .await
        .ok()
        .and_then(|v| v.into_value::<u64>().ok())
        .unwrap_or(0);
    let caches = page
        .evaluate_await(
            r#"caches.keys().then(ns => Promise.all(ns.map(n => caches.delete(n)))).then(ns => ns.length).catch(() => 0)"#,
        )
        .await
        .ok()
        .and_then(|v| v.into_value::<u64>().ok())
        .unwrap_or(0);
    Ok(Output::Json(serde_json::json!({
        "storage_cleared": storage,
        "service_workers_unregistered": workers,
        "caches_deleted": caches,
    })))
}