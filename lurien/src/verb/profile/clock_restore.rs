//! Give the host clock back to this session's pages.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "clock-restore",
    aliases: &["profile.clock-restore", "clock-clear"],
    domain: Domain::Profile,
    summary: "Read the host clock again. The live page needs no reload.",
    args: &[],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a crate::verb::Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, _args: &crate::verb::Args) -> Result<Output, Error> {
    let control = session.control()?;
    session.browser().await?;
    control.clear_clock().await?;
    Ok(Output::Json(super::clock::report(&crate::clock::Reading {
        epoch_ms: crate::clock::host_now_ms(),
        shift_ms: 0,
    })))
}
