//! Answer the file chooser a page opens from script.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "choose-files",
    aliases: &["dom.choose-files", "file-chooser"],
    domain: Domain::Dom,
    summary: "Press what opens a file chooser and give the chooser these files.",
    args: &[
        ArgSpec {
            name: "trigger",
            ty: ArgType::Str,
            required: true,
            default: None,
            help: "What to press: CSS, or a role:/text:/label:/placeholder:/testid:/ref: form.",
        },
        ArgSpec {
            name: "files",
            ty: ArgType::StrList,
            required: true,
            default: None,
            help: "Absolute local paths.",
        },
        crate::verb::TIMEOUT_ARG,
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let trigger = args.str("trigger")?;
    let files = args.str_list("files")?;
    for file in &files {
        if !std::path::Path::new(file).is_file() {
            return Err(Error::Other(format!(
                "choose-files: {file} is not a file. Give an absolute path to an existing file."
            )));
        }
    }
    let timeout_ms = crate::verb::timeout_ms(args);
    let browser = session.browser().await?;

    // Armed before the trigger is pressed: the chooser opens synchronously inside
    // the click, so arming afterwards would be too late and the native dialog
    // would already be up.
    crate::chooser::arm(browser.page()).await?;
    browser.click_within(trigger, timeout_ms).await?;
    let caught = crate::chooser::wait(browser.page(), timeout_ms).await?;

    let count = files.len();
    browser
        .page()
        .set_files(&caught.path, files)
        .await
        .map_err(|e| Error::Other(format!("choose-files {}: {e}", caught.tag)))?;
    Ok(Output::Text(format!(
        "gave {count} file(s) to the chooser {trigger} opened ({})",
        caught.tag
    )))
}
