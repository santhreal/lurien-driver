//! Copy a downloaded file out of the session directory.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "download-save",
    aliases: &["dialog.download-save"],
    domain: Domain::Dialog,
    summary: "Save a downloaded file to a path, waiting for it if it is still arriving.",
    args: &[
        ArgSpec {
            name: "path",
            ty: ArgType::Path,
            required: true,
            default: None,
            help: "Where to write the file. Parent directories are created.",
        },
        ArgSpec {
            name: "name",
            ty: ArgType::Str,
            required: false,
            default: None,
            help: "Part of the filename to save. Omit for the next file to finish.",
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
    let dest = args.path("path")?;
    let name = args.opt_str("name");
    let saved = crate::download::save(
        session,
        name.as_deref(),
        &dest,
        crate::verb::timeout_ms(args),
    )
    .await?;
    Ok(Output::Json(saved))
}
