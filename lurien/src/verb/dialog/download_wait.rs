//! Wait for a download to finish before reading the file.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "download-wait",
    aliases: &["dialog.download-wait"],
    domain: Domain::Dialog,
    summary: "Wait for a download to finish and report where the file landed.",
    args: &[
        ArgSpec {
            name: "name",
            ty: ArgType::Str,
            required: false,
            default: None,
            help: "Part of the filename to wait for. Omit for the next file to finish.",
        },
        crate::verb::TIMEOUT_ARG,
    ],
    output: OutputKind::Json,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let name = args.opt_str("name");
    let done = crate::download::wait(session, name.as_deref(), crate::verb::timeout_ms(args)).await?;
    Ok(Output::Json(done))
}
