//! Answer a request from the browser instead of the network.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// What a body is served as when the caller names no content type. A page that
/// fetches a route and gets no type at all guesses, and a guess is a flaky test.
const DEFAULT_TYPE: &str = "text/plain; charset=utf-8";

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "route-fulfil",
    aliases: &["intercept.route-fulfil"],
    domain: Domain::Intercept,
    summary: "Serve a response from the browser for every request matching a URL glob. The request never leaves the machine.",
    args: &[
        ArgSpec {
            name: "pattern",
            ty: ArgType::Str,
            required: true,
            default: None,
            help: "URL glob: * matches any run of characters, ? matches one.",
        },
        ArgSpec {
            name: "body",
            ty: ArgType::Str,
            required: false,
            default: Some(""),
            help: "Response body to serve.",
        },
        ArgSpec {
            name: "status",
            ty: ArgType::Int,
            required: false,
            default: Some("200"),
            help: "Response status, 100 to 599.",
        },
        ArgSpec {
            name: "status_text",
            ty: ArgType::Str,
            required: false,
            default: Some(""),
            help: "Reason phrase. Empty means the default for the status.",
        },
        ArgSpec {
            name: "headers",
            ty: ArgType::Str,
            required: false,
            default: Some(""),
            help: "Response headers as a JSON object, for example {\"Content-Type\":\"application/json\"}.",
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
    let mut headers = crate::route::parse_headers(SPEC.name, args.opt_str("headers").unwrap_or_default())?;
    if !headers.keys().any(|name| name.eq_ignore_ascii_case("content-type")) {
        headers.insert("Content-Type".to_string(), DEFAULT_TYPE.to_string());
    }
    let route = crate::route::Route::fulfil(
        args.str("pattern")?,
        args.i64("status", 200),
        args.opt_str("status_text").unwrap_or_default(),
        headers,
        args.opt_str("body").unwrap_or_default(),
    )?;
    // A route lives on the engine's channel observer, so there is nothing to
    // route until the engine is running.
    session.browser().await?;
    Ok(Output::Json(session.add_route(route).await?))
}
