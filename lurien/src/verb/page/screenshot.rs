//! A PNG of the viewport, the whole document, a rectangle, or one element.

use crate::error::Error;
use crate::session::Session;
use crate::shot;
use crate::verb::{
    ArgSpec, ArgType, Args, Domain, Output, OutputKind, Stability, VerbFuture, VerbSpec,
};
use runtime_foxdriver::{ShotArea, ShotOptions};

/// Registry entry. Faces read this; they never hardcode the verb.
pub static SPEC: VerbSpec = VerbSpec {
    name: "screenshot",
    aliases: &["page.screenshot"],
    domain: Domain::Page,
    summary: "Capture a PNG of the viewport, the whole page, a rectangle, or one element.",
    args: &[
        ArgSpec { name: "path", ty: ArgType::Path, required: false, default: None, help: "Write the PNG here instead of returning a byte count only." },
        ArgSpec { name: "full_page", ty: ArgType::Bool, required: false, default: Some("false"), help: "Capture the whole scrollable document instead of the viewport." },
        ArgSpec { name: "clip", ty: ArgType::Str, required: false, default: None, help: "Rectangle to capture: x,y,width,height in CSS pixels from the document top-left." },
        ArgSpec { name: "selector", ty: ArgType::Str, required: false, default: None, help: "Capture just this element. CSS, or a role:/text:/label:/placeholder:/testid:/ref: form." },
        ArgSpec { name: "frame", ty: ArgType::Str, required: false, default: None, help: "Capture this frame's own document: a frames id, index:<n>, url:<substr>, or name:<name>." },
        crate::verb::TIMEOUT_ARG,
    ],
    output: OutputKind::Png,
    stability: Stability::Stable,
    run: call,
};

fn call<'a>(session: &'a Session, args: &'a Args) -> VerbFuture<'a> {
    Box::pin(run(session, args))
}

async fn run(session: &Session, args: &Args) -> Result<Output, Error> {
    let selector = args.opt_str("selector");
    let clip = args.opt_str("clip");
    let full_page = args.bool("full_page", false);
    // Three ways of naming an area cannot all be meant at once, and picking a
    // winner silently is how a caller gets a picture of the wrong thing.
    let named: Vec<&str> = [
        selector.map(|_| "selector"),
        clip.map(|_| "clip"),
        full_page.then_some("full_page"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if named.len() > 1 {
        return Err(Error::BadArgs {
            verb: SPEC.name.to_string(),
            detail: format!(
                "{} each name a different area; pass one of them",
                named.join(" and ")
            ),
        });
    }

    let browser = session.browser().await?;
    let page = browser.page();
    let frame = args.opt_str("frame").map(str::to_string);
    let context = match &frame {
        Some(spec) => Some(
            page.resolve_frame(spec)
                .await
                .map_err(|e| Error::Other(e.to_string()))?,
        ),
        None => None,
    };
    let area = match (selector, clip) {
        (Some(selector), _) => {
            let selector = browser.deref_handle(selector).await?;
            let timeout_ms = crate::verb::timeout_ms(args);
            shot::element_region(page, context.as_ref(), &selector, timeout_ms).await?
        }
        (None, Some(clip)) => shot::parse_region(clip)?,
        (None, None) if full_page => ShotArea::Document,
        // A frame has no viewport of its own: the browser would hand back the
        // whole tab, so what the frame shows is measured inside it.
        (None, None) => match context.as_ref() {
            Some(ctx) => shot::frame_viewport_region(page, ctx).await?,
            None => ShotArea::Viewport,
        },
    };
    let png = page
        .screenshot_with(&ShotOptions { area, frame })
        .await
        .map_err(|e| Error::Other(e.to_string()))?;
    if let Some(path) = args.opt_path("path") {
        std::fs::write(&path, &png)
            .map_err(|e| Error::Other(format!("write {}: {e}", path.display())))?;
    }
    Ok(Output::Png(png))
}
