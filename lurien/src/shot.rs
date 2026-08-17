//! What a picture of a page covers.
//!
//! A viewport capture answers "what would a person see right now", and that is
//! the wrong answer for most of what a caller wants a picture for: a receipt
//! that runs past the fold, one component in a design review, a captcha tile
//! inside a cross-origin iframe. Those are three areas of the same page, so they
//! are three ways of describing an area rather than three screenshot verbs.
//!
//! | Area | Meaning |
//! |---|---|
//! | viewport | what is on screen now, the default |
//! | document | the whole scrollable page, however tall |
//! | region | a rectangle in CSS pixels from the document's top-left |
//! | element | the rectangle one element occupies |
//!
//! Every one of them is a single browser-side render. Nothing scrolls, nothing
//! is stitched, and no page script observes the capture: a sticky header appears
//! once instead of once per stitched band, and a scroll-triggered animation is
//! not started by the act of taking the picture. An element or a region below
//! the fold needs no scrolling for the same reason, so the page is left exactly
//! as the caller left it.

use crate::error::Error;
use crate::locator;
use runtime_foxdriver::{FrameId, Page, ShotArea};

/// Verb name used in argument errors, so a face reports the same name the caller
/// typed.
const VERB: &str = "screenshot";

/// Parse a `x,y,width,height` rectangle in CSS pixels.
///
/// Four numbers, because a rectangle is four numbers and a caller writing one on
/// a command line should not have to write four flags.
pub fn parse_region(spec: &str) -> Result<ShotArea, Error> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    let bad = |detail: String| Error::BadArgs {
        verb: VERB.to_string(),
        detail,
    };
    if parts.len() != 4 {
        return Err(bad(format!(
            "clip {spec:?} has {} part(s); write it as x,y,width,height in CSS pixels",
            parts.len()
        )));
    }
    let mut n = [0.0_f64; 4];
    for (slot, raw) in n.iter_mut().zip(&parts) {
        *slot = raw.parse::<f64>().map_err(|_| {
            bad(format!(
                "clip {spec:?} has {raw:?} where a number belongs; write it as x,y,width,height in CSS pixels"
            ))
        })?;
        if !slot.is_finite() {
            return Err(bad(format!(
                "clip {spec:?} is not a finite rectangle; write it as x,y,width,height in CSS pixels"
            )));
        }
    }
    if n[2] <= 0.0 || n[3] <= 0.0 {
        return Err(bad(format!(
            "clip {spec:?} is {}x{}; give a positive width and height",
            n[2], n[3]
        )));
    }
    Ok(ShotArea::Region {
        x: n[0],
        y: n[1],
        width: n[2],
        height: n[3],
    })
}

/// Measure the rectangle `selector` occupies, in the document that owns it.
///
/// The selector language is the same one every act verb takes, resolved in the
/// target document, so `role:` and `text:` describe an element to photograph as
/// readily as one to click. The measurement is a read: it does not scroll the
/// element into view, because a document-origin capture does not need it to be
/// on screen and moving the page would be a side effect of taking a picture.
pub async fn element_region(
    page: &Page,
    context: Option<&FrameId>,
    selector: &str,
    timeout_ms: u64,
) -> Result<ShotArea, Error> {
    let resolved = match context {
        Some(ctx) => locator::resolve_present_in(page, ctx, selector, timeout_ms).await?,
        None => locator::resolve_present(page, selector, timeout_ms).await?,
    };
    let script = format!(
        "(() => {{ const el = document.querySelector({css});\n\
         if (!el) return \"null\";\n\
         const r = el.getBoundingClientRect();\n\
         const d = document.documentElement;\n\
         const sx = window.scrollX || d.scrollLeft || 0;\n\
         const sy = window.scrollY || d.scrollTop || 0;\n\
         return JSON.stringify({{x: r.left + sx, y: r.top + sy, width: r.width, height: r.height}}); }})()",
        css = serde_json::Value::String(resolved.css.clone()),
    );
    let rect = measure(page, context, &script, selector).await?;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(Error::Unresolved {
            selector: selector.to_string(),
            detail: format!("resolved to {}, which occupies no pixels", resolved.css),
            waited_ms: resolved.waited_ms,
            action: "Photograph an element that has a box: a collapsed or display:none element has nothing to capture."
                .to_string(),
        });
    }
    Ok(rect.into())
}

/// The rectangle a frame currently shows of its own document.
///
/// As far as the browser's capture command is concerned a frame has no viewport:
/// asking for the viewport while naming a frame photographs the whole tab. What
/// the caller meant is what is on screen inside that frame, so the rectangle is
/// measured in the frame and captured as a region of the frame's document. A
/// document smaller than the frame that shows it is captured at its own size,
/// because the empty band around it is not part of any document.
///
/// The measurement is the frame document's layout viewport, not
/// `window.innerWidth`: a persona spoofs window dimensions to the top window's
/// size, so inside an iframe `innerWidth` describes the tab rather than the
/// frame, while `documentElement.clientWidth` is the box the frame actually
/// lays its document out in.
pub async fn frame_viewport_region(page: &Page, context: &FrameId) -> Result<ShotArea, Error> {
    let script = "(() => { const d = document.documentElement;\n\
         const w = Math.min(d.clientWidth || window.innerWidth || 0, d.scrollWidth || Infinity);\n\
         const h = Math.min(d.clientHeight || window.innerHeight || 0, d.scrollHeight || Infinity);\n\
         return JSON.stringify({x: window.scrollX || 0, y: window.scrollY || 0, width: w, height: h}); })()";
    let rect = measure(page, Some(context), script, "frame").await?;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(Error::Other(format!(
            "frame shows {}x{} of its document; capture it with full_page, or wait for the frame to lay out",
            rect.width, rect.height
        )));
    }
    Ok(rect.into())
}

/// A measured rectangle, in the coordinates of the document that reported it.
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl From<Rect> for ShotArea {
    fn from(r: Rect) -> Self {
        Self::Region {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// Run a measuring script in one document and read its rectangle. `what` names
/// the thing being measured, so a failure says whose measurement was lost.
async fn measure(
    page: &Page,
    context: Option<&FrameId>,
    script: &str,
    what: &str,
) -> Result<Rect, Error> {
    let eval = match context {
        Some(ctx) => page.evaluate_in_context(script.to_string(), ctx).await,
        None => page.evaluate(script.to_string()).await,
    };
    let raw = eval
        .map_err(|e| Error::Other(format!("{what}: measure failed: {e}")))?
        .into_value::<String>()
        .map_err(|e| Error::Other(format!("{what}: measure returned no answer: {e}")))?;
    let rect: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| Error::Other(format!("{what}: measure answer unreadable: {e}")))?;
    let num = |key: &str| rect[key].as_f64().unwrap_or(0.0);
    Ok(Rect {
        x: num("x"),
        y: num("y"),
        width: num("width"),
        height: num("height"),
    })
}

/// Pixel size of a PNG, read from its IHDR chunk.
///
/// Every face reports the size of a capture, so a caller can tell a full-page
/// shot from a viewport one without opening the file, and a test can assert the
/// geometry it asked for. `None` means the bytes are not a PNG, which is a fact
/// about the bytes rather than an error.
#[must_use]
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    // Signature, 4-byte length, "IHDR", then width and height.
    if bytes.len() < 33 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| {
        u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    Some((word(16), word(20)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_four_numbers_in_any_spacing() {
        assert_eq!(
            parse_region(" 10, 20 ,300,40 ").unwrap(),
            ShotArea::Region {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 40.0
            }
        );
    }

    #[test]
    fn a_rectangle_with_the_wrong_arity_names_the_shape_it_wanted() {
        let err = parse_region("10,20,30").unwrap_err().to_string();
        assert!(err.contains("3 part(s)"), "{err}");
        assert!(err.contains("x,y,width,height"), "{err}");
    }

    #[test]
    fn a_rectangle_with_a_word_in_it_says_which_word() {
        let err = parse_region("10,20,wide,40").unwrap_err().to_string();
        assert!(err.contains("\"wide\""), "{err}");
    }

    #[test]
    fn a_flat_rectangle_is_refused_before_the_browser_sees_it() {
        for spec in ["10,20,0,40", "10,20,30,-1"] {
            let err = parse_region(spec).unwrap_err().to_string();
            assert!(
                err.contains("positive width and height"),
                "{spec}: {err}"
            );
        }
    }

    #[test]
    fn png_geometry_comes_from_the_header() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1280u32.to_be_bytes());
        png.extend_from_slice(&2400u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(png_size(&png), Some((1280, 2400)));
    }

    #[test]
    fn bytes_that_are_not_a_png_report_no_size_rather_than_a_wrong_one() {
        assert_eq!(png_size(b"not an image at all, not even close"), None);
        assert_eq!(png_size(&[0x89, b'P', b'N', b'G']), None);
    }
}
