//! Move this session's clock to a time of your choosing.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "clock-set",
    aliases: &["profile.clock-set"],
    domain: Domain::Profile,
    summary: "Serve a different time. A page reads it from its first script on, and keeps ticking at the host's rate.",
    args: &[ArgSpec {
        name: "time",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "Milliseconds since the epoch, or a time like 2033-05-18T03:33:20Z.",
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
    let epoch_ms = crate::clock::parse_time(args.str("time")?)?;
    // The engine has to be running: a clock is installed in the compartment of
    // the page that reads it, so there is nothing to move until there is a page.
    session.browser().await?;
    let shift_ms = control.set_clock(epoch_ms).await?;
    Ok(Output::Json(super::clock::report(&crate::clock::Reading {
        epoch_ms: crate::clock::host_now_ms() + shift_ms,
        shift_ms,
    })))
}
