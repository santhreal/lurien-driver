//! Answer an open prompt.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "dialog",
    aliases: &["dialog.answer"],
    domain: Domain::Dialog,
    summary: "Accept or dismiss the open dialog, optionally with prompt text.",
    args: &[
        ArgSpec { name: "action", ty: ArgType::Str, required: true, default: None, help: "accept or dismiss." },
        ArgSpec { name: "text", ty: ArgType::Str, required: false, default: None, help: "Text for a prompt() dialog." },
        ArgSpec { name: "frame", ty: ArgType::Str, required: false, default: None, help: "Frame target owning the dialog. Defaults to the main document." },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let action = args.str("action")?.trim().to_ascii_lowercase();
    let accept = match action.as_str() {
        "accept" => true,
        "dismiss" => false,
        other => {
            return Err(Error::BadArgs {
                verb: "dialog".into(),
                detail: format!("action must be accept or dismiss, got {other:?}"),
            })
        }
    };
    let telemetry = session.telemetry().await?;
    if telemetry.dialogs.open_dialogs().await.is_empty() {
        // Speculative answers are common; say so instead of surfacing a raw
        // "no such alert" from the driver.
        return Ok(Output::Text("no dialog is open".into()));
    }
    let browser = session.browser().await?;
    let context = match args.opt_str("frame") {
        Some(spec) => Some(
            browser
                .page()
                .resolve_frame(spec)
                .await
                .map_err(|e| Error::Other(format!("dialog frame {spec}: {e}")))?,
        ),
        None => None,
    };
    browser
        .page()
        .handle_user_prompt(context.as_ref(), accept, args.opt_str("text"))
        .await
        .map_err(|e| Error::Other(format!("dialog {action}: {e}")))?;
    Ok(Output::Text(format!(
        "dialog {}",
        if accept { "accepted" } else { "dismissed" }
    )))
}