//! The page as a list of addressable nodes, or as text, or as source.

use crate::error::Error;
use crate::session::Session;
use crate::snapshot::DEFAULT_LIMIT;
use crate::verb::{
    ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec,
};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "snapshot",
    aliases: &["page.snapshot"],
    domain: Domain::Page,
    summary: "The page as roles, names and handles. Handles act as `ref:eN` selectors.",
    args: &[
        ArgSpec {
            name: "format",
            ty: ArgType::Str,
            required: false,
            default: Some("nodes"),
            help: "nodes (default, addressable roles and handles), text, or source.",
        },
        ArgSpec {
            name: "limit",
            ty: ArgType::Int,
            required: false,
            default: None,
            help: "Maximum nodes to report. Default 200; the answer says what it dropped.",
        },
    ],
    output: OutputKind::Text,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let format = args.opt_str("format").unwrap_or("nodes");
    let limit = usize::try_from(args.u64("limit", DEFAULT_LIMIT as u64).max(1))
        .unwrap_or(DEFAULT_LIMIT);
    let browser = session.browser().await?;
    let text = match format {
        // The node list is the default because it is what an agent can act on:
        // page source is mostly markup no verb accepts, and it changes on every
        // redesign even when the page still does the same thing.
        "nodes" => browser.snapshot(limit).await?.render(),
        "text" => browser.snapshot_text().await?,
        "source" => browser.source().await?,
        other => {
            return Err(Error::Other(format!(
                "unknown snapshot format {other:?}. Use nodes, text, or source."
            )))
        }
    };
    Ok(Output::Text(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default is the representation an agent acts from. A release that
    /// quietly went back to page source would cost every caller tokens on markup
    /// and lose the handles, so the default is pinned.
    #[test]
    fn the_default_format_is_the_node_list() {
        let format = SPEC
            .args
            .iter()
            .find(|a| a.name == "format")
            .expect("the verb takes a format");
        assert_eq!(format.default, Some("nodes"));
        assert!(!format.required, "a caller should not have to name the default");
    }

    /// Source stays reachable: a caller debugging a page needs the markup, and
    /// having to leave the tool for it is the reason wrappers get written.
    #[test]
    fn source_and_text_are_still_reachable() {
        let help = SPEC
            .args
            .iter()
            .find(|a| a.name == "format")
            .map(|a| a.help)
            .unwrap_or_default();
        assert!(help.contains("source"), "{help}");
        assert!(help.contains("text"), "{help}");
    }
}
