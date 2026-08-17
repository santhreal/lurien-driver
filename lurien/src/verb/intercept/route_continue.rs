//! Send a request, with the headers changed.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "route-continue",
    aliases: &["intercept.route-continue"],
    domain: Domain::Intercept,
    summary: "Edit the request headers of every request matching a URL glob, then send it. Use * to reach every request.",
    args: &[
        ArgSpec {
            name: "pattern",
            ty: ArgType::Str,
            required: false,
            default: Some("*"),
            help: "URL glob: * matches any run of characters, ? matches one.",
        },
        ArgSpec {
            name: "headers",
            ty: ArgType::Str,
            required: false,
            default: Some(""),
            help: "Request headers to set, as a JSON object, for example {\"X-Trace\":\"1\"}.",
        },
        ArgSpec {
            name: "remove",
            ty: ArgType::StrList,
            required: false,
            default: Some(""),
            help: "Request header names to drop, for example Referer.",
        },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let headers = crate::route::parse_headers(SPEC.name, args.opt_str("headers").unwrap_or_default())?;
    let remove: Vec<String> = args
        .opt_str_list("remove")?
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    let route = crate::route::Route::cont(
        args.opt_str("pattern").filter(|p| !p.trim().is_empty()).unwrap_or("*"),
        headers,
        remove,
    )?;
    // Headers are set on the channel in the engine's parent process, so there is
    // nothing to edit until the engine is running.
    session.browser().await?;
    Ok(Output::Json(session.add_route(route).await?))
}
