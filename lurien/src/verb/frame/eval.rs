//! Evaluate JavaScript in a named context.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "eval",
    aliases: &["frame.eval"],
    domain: Domain::Frame,
    summary: "Evaluate JavaScript in the main document or a named frame.",
    args: &[
        ArgSpec { name: "script", ty: ArgType::Str, required: true, default: None, help: "Expression to evaluate." },
        ArgSpec { name: "frame", ty: ArgType::Str, required: false, default: None, help: "Frame target: id, url substring, name, or main." },
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let script = args.str("script")?;
    let browser = session.browser().await?;
    let result = match args.opt_str("frame") {
        Some(spec) => browser
            .page()
            .eval_in_frame(spec, script)
            .await
            .map_err(|e| Error::Other(format!("eval in {spec}: {e}")))?,
        None => browser
            .page()
            .evaluate(script)
            .await
            .map_err(|e| Error::Other(format!("eval: {e}")))?,
    };
    let value = result
        .into_value::<serde_json::Value>()
        .unwrap_or(serde_json::Value::Null);
    Ok(Output::Json(value))
}