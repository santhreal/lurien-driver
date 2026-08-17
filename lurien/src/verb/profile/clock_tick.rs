//! Move this session's clock by an interval.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "clock-tick",
    aliases: &["profile.clock-tick"],
    domain: Domain::Profile,
    summary: "Move the clock forwards or back by an interval. Readers see the jump; pending timers do not fire early.",
    args: &[ArgSpec {
        name: "ms",
        ty: ArgType::Int,
        required: true,
        default: None,
        help: "Milliseconds to add. Negative moves the clock back.",
    }],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let control = session.control()?;
    let ms = args.i64("ms", 0);
    session.browser().await?;
    let shift_ms = control.tick_clock(ms).await?;
    Ok(Output::Json(super::clock::report(&crate::clock::Reading {
        epoch_ms: crate::clock::host_now_ms() + shift_ms,
        shift_ms,
    })))
}
