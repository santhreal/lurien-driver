//! Stop a request from being sent at all.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "route-abort",
    aliases: &["intercept.route-abort"],
    domain: Domain::Intercept,
    summary: "Cancel every request matching a URL glob. The page sees a network error, as it would offline.",
    args: &[ArgSpec {
        name: "pattern",
        ty: ArgType::Str,
        required: true,
        default: None,
        help: "URL glob: * matches any run of characters, ? matches one.",
    }],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let route = crate::route::Route::abort(args.str("pattern")?)?;
    // The cancel happens on the channel in the engine's parent process, so
    // there is nothing to cancel until the engine is running.
    session.browser().await?;
    Ok(Output::Json(session.add_route(route).await?))
}
