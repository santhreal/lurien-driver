//! Attach local files to a file input.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "upload",
    aliases: &["dom.upload"],
    domain: Domain::Dom,
    summary: "Attach files to a file input.",
    args: &[
        ArgSpec { name: "selector", ty: ArgType::Str, required: true, default: None, help: "CSS selector of the file input." },
        ArgSpec { name: "files", ty: ArgType::StrList, required: true, default: None, help: "Absolute local paths." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let selector = args.str("selector")?;
    let files = args.str_list("files")?;
    for file in &files {
        if !std::path::Path::new(file).is_file() {
            return Err(Error::Other(format!("upload: {file} is not a file")));
        }
    }
    let count = files.len();
    session
        .browser()
        .await?
        .page()
        .set_files(selector, files)
        .await
        .map_err(|e| Error::Other(format!("upload {selector}: {e}")))?;
    Ok(Output::Text(format!("attached {count} file(s) to {selector}")))
}