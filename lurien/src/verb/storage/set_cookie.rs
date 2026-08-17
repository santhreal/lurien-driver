//! Write one cookie through BiDi storage.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "set-cookie",
    aliases: &["storage.set_cookie"],
    domain: Domain::Storage,
    summary: "Set one cookie via BiDi storage.",
    args: &[
        ArgSpec { name: "name", ty: ArgType::Str, required: true, default: None, help: "Cookie name." },
        ArgSpec { name: "value", ty: ArgType::Str, required: true, default: None, help: "Cookie value." },
        ArgSpec { name: "domain", ty: ArgType::Str, required: true, default: None, help: "Cookie domain, with or without a leading dot." },
        ArgSpec { name: "path", ty: ArgType::Str, required: false, default: None, help: "Cookie path. Defaults to the browser default." },
        ArgSpec { name: "expires", ty: ArgType::Int, required: false, default: None, help: "Unix expiry in seconds. Omit for a session cookie." },
        ArgSpec { name: "secure", ty: ArgType::Bool, required: false, default: None, help: "Set the Secure attribute." },
        ArgSpec { name: "http_only", ty: ArgType::Bool, required: false, default: None, help: "Set the HttpOnly attribute." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let name = args.str("name")?;
    let value = args.str("value")?;
    let domain = args.str("domain")?;
    let path = args.opt_str("path");
    let expires = args.as_map().get("expires").and_then(serde_json::Value::as_u64);
    let secure = args.as_map().get("secure").and_then(serde_json::Value::as_bool);
    let http_only = args
        .as_map()
        .get("http_only")
        .and_then(serde_json::Value::as_bool);
    session
        .browser()
        .await?
        .page()
        .set_cookie(name, value, domain, path, expires, secure, http_only, None)
        .await
        .map_err(|e| Error::Other(format!("set-cookie {name}: {e}")))?;
    Ok(Output::Text(format!("set {name} for {domain}")))
}