//! Viewport PNG. Writes a file when `path` is given.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "screenshot",
    aliases: &["page.screenshot"],
    domain: Domain::Page,
    summary: "Capture a viewport PNG. Writes the file when path is given.",
    args: &[
        ArgSpec { name: "path", ty: ArgType::Path, required: false, default: None, help: "Write the PNG here instead of returning a byte count only." },
    ],
    output: OutputKind::Png,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let png = session.browser().await?.screenshot().await?;
    if let Some(path) = args.opt_path("path") {
        std::fs::write(&path, &png)
            .map_err(|e| Error::Other(format!("write {}: {e}", path.display())))?;
    }
    Ok(Output::Png(png))
}