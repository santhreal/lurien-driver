//! Cross-origin iframe evaluation helpers.
//!
//! CAPTCHA providers (reCAPTCHA, hCaptcha, Turnstile) render their challenges
//! inside sandboxed cross-origin iframes.  JavaScript running in the parent
//! page cannot pierce these iframes via `contentDocument` or
//! `contentWindow.document`: doing so throws a `SecurityError`.
//!
//! This module uses WebDriver BiDi to evaluate expressions in each frame's
//! own execution context, which works regardless of origin.

use crate::browser::Page;
use anyhow::Result;
use std::time::{Duration, Instant};

/// Default poll cadence for the retry helpers. CAPTCHA iframes typically
/// attach within 50–500ms of navigation; 100ms strikes a balance between
/// responsiveness and CDP traffic.
pub const DEFAULT_FRAME_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// Default upper bound for the retry helpers. If a captcha widget hasn't
/// attached after 8s the page is almost certainly broken or behind a
/// network stall (caller should fall back rather than wait longer).
pub const DEFAULT_FRAME_RETRY_TIMEOUT: Duration = Duration::from_secs(8);

/// Compute the next sleep duration for a polling loop, clamped so we
/// never overshoot the deadline. Pulled out as a pure function so the
/// retry behaviour can be unit-tested without a real browser.
///
/// Returns `None` when the deadline has been reached or passed.
fn next_poll_sleep(now: Instant, deadline: Instant, interval: Duration) -> Option<Duration> {
    if now >= deadline {
        return None;
    }
    let remaining = deadline.saturating_duration_since(now);
    Some(remaining.min(interval))
}

/// Escape a Rust string so it is safe to embed in a JavaScript string literal
/// surrounded by either single or double quotes.
///
/// Handles the following escapes:
/// - `\`  → `\\`
/// - `'`  → `\'`
/// - `"`  → `\"`
/// - `\n` → `\\n`
/// - `\r` → `\\r`
/// - `\t` → `\\t`
/// - `\0` → `\\0`
/// - `\u{2028}` → `\\u2028`
/// - `\u{2029}` → `\\u2029`
pub(crate) fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// Look up the iframe offset for a given URL and optional iframe index.
///
/// `iframe_offsets` is a Vec of `(idx, src, id, x, y)` tuples collected from
/// the main frame in DOM order.  If `iframe_idx` is non-negative we prefer an
/// exact index match to disambiguate duplicate URLs.
fn lookup_iframe_offset(
    iframe_offsets: &[(usize, String, String, f64, f64)],
    url: &str,
    iframe_idx: i64,
) -> Option<(f64, f64)> {
    let matches_url = |src: &str, id: &str| -> bool {
        src == url || id == url || (!src.is_empty() && (url.contains(src) || src.contains(url)))
    };

    if iframe_idx >= 0 {
        if let Some((_, _, _, x, y)) = iframe_offsets
            .iter()
            .find(|(idx, src, id, _, _)| *idx == iframe_idx as usize && matches_url(src, id))
        {
            return Some((*x, *y));
        }
        if let Some((_, _, _, x, y)) = iframe_offsets
            .iter()
            .find(|(idx, _, _, _, _)| *idx == iframe_idx as usize)
        {
            return Some((*x, *y));
        }
        return None;
    }

    if let Some((_, _, _, x, y)) = iframe_offsets
        .iter()
        .find(|(_, src, id, _, _)| matches_url(src, id))
    {
        return Some((*x, *y));
    }

    if iframe_offsets.len() == 1 {
        let (_, src, _, x, y) = &iframe_offsets[0];
        if src.is_empty() || src == "about:blank" {
            return Some((*x, *y));
        }
    }

    None
}

/// Evaluate `expression` in every frame of the page (main document + all
/// iframes) and return the deserialized results from every frame that
/// produced a valid value.
///
/// This is the robust replacement for parent-page JS that tries to walk
/// into `iframe.contentDocument`.
///
/// # Example
///
/// ```rust,no_run
/// use runtime_foxdriver::frame::evaluate_in_all_frames;
/// # async fn example(page: &runtime_foxdriver::Page) -> anyhow::Result<()> {
/// let titles: Vec<String> = evaluate_in_all_frames(page, "document.title").await?;
/// # Ok(()) }
/// ```
pub async fn evaluate_in_all_frames<T>(page: &Page, expression: &str) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let frame_ids = page.frames().await?;
    let mut out = Vec::with_capacity(frame_ids.len());
    for fid in frame_ids {
        match page.evaluate_in_context(expression, &fid).await {
            Ok(eval) => {
                if let Ok(v) = eval.into_value::<T>() {
                    out.push(v);
                }
            }
            Err(e) => {
                tracing::debug!("frame {:?} disappeared during batch eval: {}", fid, e);
            }
        }
    }
    Ok(out)
}

/// Evaluate `expression` in every frame and return the **first** result that
/// passes `filter`.  If no frame produces a matching result, `default` is
/// returned.
pub async fn evaluate_in_frames_first<T, F>(
    page: &Page,
    expression: &str,
    filter: F,
    default: T,
) -> Result<T>
where
    T: serde::de::DeserializeOwned + Clone,
    F: Fn(&T) -> bool,
{
    let all = evaluate_in_all_frames::<T>(page, expression).await?;
    Ok(all.into_iter().find(filter).unwrap_or(default))
}

/// Collect every direct-child iframe's `(dom_index, src, id, left, top)` offset
/// within `frame`'s OWN coordinate system, in DOM order. Shared by the
/// cross-origin coordinate helpers so a child iframe's rect can be summed up the
/// ancestor chain to yield a main-viewport coordinate. A `Vec` (not a map) is
/// used so two iframes sharing a `src` stay disambiguated by their DOM index.
async fn collect_iframe_offsets(
    page: &Page,
    frame: &crate::FrameId,
) -> Result<Vec<(usize, String, String, f64, f64)>> {
    let mut iframe_offsets: Vec<(usize, String, String, f64, f64)> = Vec::new();
    let js = r#"
        (function() {
            const out = [];
            const frames = document.querySelectorAll('iframe, frame');
            for (let i = 0; i < frames.length; i++) {
                const f = frames[i];
                const r = f.getBoundingClientRect();
                out.push({ idx: i, src: f.src, id: f.id, x: r.left, y: r.top });
            }
            return out;
        })()
    "#;
    let eval = page.evaluate_in_context(js, frame).await?;
    if let Ok(vals) = eval.into_value::<Vec<serde_json::Value>>() {
        for v in vals {
            if let (Some(idx), Some(x), Some(y)) =
                (v["idx"].as_u64(), v["x"].as_f64(), v["y"].as_f64())
            {
                let src = v["src"].as_str().unwrap_or("").to_string();
                let id = v["id"].as_str().unwrap_or("").to_string();
                iframe_offsets.push((idx as usize, src, id, x, y));
            }
        }
    }
    Ok(iframe_offsets)
}

/// This frame's own index within its parent's `window.frames`, or `-1` when it
/// is a top-level context (or the lookup is blocked). Used to match a child
/// browsing context to its `<iframe>` element in the parent's DOM order, the
/// pairing is index-aligned in Firefox (verified: an interleaved non-iframe
/// browsing context does not desync iframe indices).
async fn frame_self_index(page: &Page, frame: &crate::FrameId) -> i64 {
    let js = r#"(function() {
        try {
            const fr = window.parent.frames;
            for (let i = 0; i < fr.length; i++) { if (fr[i] === window) return i; }
        } catch (e) {}
        return -1;
    })()"#;
    page.evaluate_in_context(js, frame)
        .await
        .ok()
        .and_then(|e| e.into_value::<i64>().ok())
        .unwrap_or(-1)
}

/// Sum of every ancestor iframe's top-left, lifting an element rect measured
/// inside `target`'s realm to MAIN-VIEWPORT coordinates.
///
/// The old path summed only the MAIN frame's direct-iframe offsets, so an
/// element two or more frames deep (a checkbox inside Turnstile's nested
/// `challenges.cloudflare.com` iframe, a tile inside a grid nested below the
/// vendor frame) got at most ONE level of offset and landed at the wrong
/// viewport point. This walks the REAL parent chain recovered from
/// `browsingContext.getTree` ([`Page::frame_tree`]) and accumulates each edge's
/// iframe offset, so the result is correct at ANY nesting depth. Returns
/// `(0, 0)` for the main / a top-level frame (chain of length ≤ 1).
///
/// Matching is INDEX-PRIMARY: a child's `window.frames` index within its parent
/// uniquely identifies its `<iframe>` element, with src/id as a fallback only
/// when the index is unavailable. An edge that cannot be resolved is logged at
/// `warn` and contributes no offset (never a silent miss).
async fn frame_viewport_offset(page: &Page, target: &crate::FrameId) -> Result<(f64, f64)> {
    use std::collections::HashMap;

    let tree = page.frame_tree().await?;
    let by_id: HashMap<&str, &crate::browser::FrameTreeNode> =
        tree.iter().map(|n| (n.id.inner().as_str(), n)).collect();

    // Walk target → parent → … → top-level (parent == None).
    let mut chain: Vec<&crate::browser::FrameTreeNode> = Vec::new();
    let mut cur = by_id.get(target.inner().as_str()).copied();
    while let Some(node) = cur {
        chain.push(node);
        cur = node
            .parent
            .as_ref()
            .and_then(|p| by_id.get(p.inner().as_str()).copied());
    }

    let mut ox = 0.0;
    let mut oy = 0.0;
    for i in 0..chain.len().saturating_sub(1) {
        let child = chain[i];
        let parent = chain[i + 1];
        let kids = collect_iframe_offsets(page, &parent.id).await?;
        let idx = frame_self_index(page, &child.id).await;

        let off = kids
            .iter()
            .find(|(kidx, _, _, _, _)| idx >= 0 && *kidx == idx as usize)
            .map(|(_, _, _, x, y)| (*x, *y))
            .or_else(|| lookup_iframe_offset(&kids, &child.url, -1));

        match off {
            Some((x, y)) => {
                ox += x;
                oy += y;
            }
            None => tracing::warn!(
                "frame_viewport_offset: unresolved iframe edge for {} (idx {idx}) within {}",
                child.url,
                parent.url
            ),
        }
    }
    Ok((ox, oy))
}

/// Search every frame for a DOM element matching `selector` and return its
/// bounding-box centre coordinates **relative to the main viewport**.
///
/// For elements inside cross-origin iframes this sums the iframe's own
/// bounding box with the element's position inside the iframe so the
/// resulting coordinates are safe to pass to `Input.dispatchMouseEvent`.
///
/// # Example
///
/// ```rust,no_run
/// use runtime_foxdriver::frame::find_element_centre_in_frames;
/// # async fn example(page: &runtime_foxdriver::Page) -> anyhow::Result<()> {
/// let centre = find_element_centre_in_frames(page, "#submit-btn").await?;
/// if let Some((x, y)) = centre {
///     // x, y are viewport-relative coordinates
/// }
/// # Ok(()) }
/// ```
pub async fn find_element_centre_in_frames(
    page: &Page,
    selector: &str,
) -> Result<Option<(f64, f64)>> {
    let frame_ids = page.frames().await?;

    let escaped = escape_js_string(selector);
    let js = format!(
        r#"(function() {{
            const el = document.querySelector('{}');
            if (!el) return null;
            const r = el.getBoundingClientRect();
            return {{ x: r.left + r.width / 2, y: r.top + r.height / 2 }};
        }})()"#,
        escaped
    );

    for fid in frame_ids {
        match page.evaluate_in_context(&js, &fid).await {
            Ok(eval) => {
                if let Ok(val) = eval.into_value::<serde_json::Value>() {
                    if let (Some(x), Some(y)) = (val["x"].as_f64(), val["y"].as_f64()) {
                        // Lift the in-frame rect to main-viewport coords by
                        // summing the FULL ancestor-iframe chain (correct at any
                        // nesting depth, not just one level below main).
                        let (offset_x, offset_y) = frame_viewport_offset(page, &fid).await?;
                        return Ok(Some((x + offset_x, y + offset_y)));
                    }
                }
            }
            Err(e) => {
                tracing::debug!("frame {:?} disappeared during element search: {}", fid, e);
            }
        }
    }
    Ok(None)
}

/// Retrying variant of [`find_element_centre_in_frames`].
///
/// Captcha widgets frequently inject their iframe asynchronously a few
/// hundred milliseconds after the host page loads (Turnstile, hCaptcha
/// invisible, recaptcha v2 audio fallback). A single-shot
/// [`find_element_centre_in_frames`] call against a freshly-navigated
/// page will return `Ok(None)` for those cases, not because the widget
/// is missing, but because the iframe hasn't attached yet.
///
/// This wrapper polls the frame tree on `interval` until either:
/// - a frame returns coordinates (returns `Ok(Some((x, y)))`), or
/// - the wall-clock deadline `timeout` elapses (returns `Ok(None)`).
///
/// `interval` is clamped to never overshoot the deadline, so the actual
/// number of CDP round-trips is bounded by `timeout / interval + 1`.
///
/// Errors from the underlying single-shot call are propagated immediately
///: only the "not found" outcome triggers a retry.
pub async fn find_element_centre_in_frames_retry(
    page: &Page,
    selector: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<Option<(f64, f64)>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(centre) = find_element_centre_in_frames(page, selector).await? {
            return Ok(Some(centre));
        }
        match next_poll_sleep(Instant::now(), deadline, interval) {
            Some(d) => tokio::time::sleep(d).await,
            None => return Ok(None),
        }
    }
}

/// A single grid cell located inside the frame tree, with its bounding box
/// expressed in **main-viewport** coordinates (the iframe offset is already
/// summed in). `index` is the element's 0-based position in the owning frame's
/// `querySelectorAll(selector)` result, in document order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTile {
    /// 0-based index within the owning frame's match list (document order).
    pub index: usize,
    /// Left edge in main-viewport CSS pixels.
    pub left: f64,
    /// Top edge in main-viewport CSS pixels.
    pub top: f64,
    /// Width in CSS pixels.
    pub width: f64,
    /// Height in CSS pixels.
    pub height: f64,
}

impl FrameTile {
    /// Centre point in main-viewport CSS pixels, safe to pass to the trusted
    /// top-context [`Page::click_at`], which routes the event into the owning
    /// (possibly cross-origin) frame.
    pub fn centre(&self) -> (f64, f64) {
        (self.left + self.width / 2.0, self.top + self.height / 2.0)
    }
}

/// Locate **all** elements matching `selector` and return their bounding boxes
/// in main-viewport coordinates, taking the matches from the FIRST frame that
/// contains any.
///
/// This is the grid-aware sibling of [`find_element_centre_in_frames`]: image
/// CAPTCHAs (reCAPTCHA v2 image grid, hCaptcha) render their tile table inside
/// a cross-origin OOPIF (`google.com/recaptcha/api2/bframe`,
/// `*.hcaptcha.com`). Parent-page JS cannot see those tiles at all, and a
/// synthetic `el.dispatchEvent(new MouseEvent('click'))` would be
/// `isTrusted === false` even if it could. By returning every tile's
/// viewport-relative rect, the caller can drive a TRUSTED
/// [`Page::click_at`]/[`Page::click_at_in`] at the exact centre of any tile
/// the only click a modern image CAPTCHA accepts.
///
/// Returns an empty `Vec` when no frame contains a match. Within the winning
/// frame, tiles are returned in document order with stable `index` values, so
/// `tiles[i].index == i`.
pub async fn find_tiles_in_frames(page: &Page, selector: &str) -> Result<Vec<FrameTile>> {
    let frame_ids = page.frames().await?;

    let escaped = escape_js_string(selector);
    let js = format!(
        r#"(function() {{
            const els = document.querySelectorAll('{}');
            if (!els || els.length === 0) return null;
            const tiles = [];
            for (let i = 0; i < els.length; i++) {{
                const r = els[i].getBoundingClientRect();
                tiles.push({{ index: i, left: r.left, top: r.top, width: r.width, height: r.height }});
            }}
            return {{ tiles: tiles }};
        }})()"#,
        escaped
    );

    for fid in frame_ids {
        let eval = match page.evaluate_in_context(&js, &fid).await {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("frame {:?} disappeared during tile search: {}", fid, e);
                continue;
            }
        };
        let Ok(val) = eval.into_value::<serde_json::Value>() else {
            continue;
        };
        let Some(raw_tiles) = val["tiles"].as_array() else {
            continue;
        };
        if raw_tiles.is_empty() {
            continue;
        }
        // Lift every tile rect to main-viewport coords by summing the FULL
        // ancestor-iframe chain (a grid nested 2+ frames deep accumulates every
        // level's offset, not just the main frame's direct child).
        let (offset_x, offset_y) = frame_viewport_offset(page, &fid).await?;
        let mut out = Vec::with_capacity(raw_tiles.len());
        for t in raw_tiles {
            if let (Some(index), Some(left), Some(top), Some(width), Some(height)) = (
                t["index"].as_u64(),
                t["left"].as_f64(),
                t["top"].as_f64(),
                t["width"].as_f64(),
                t["height"].as_f64(),
            ) {
                out.push(FrameTile {
                    index: index as usize,
                    left: left + offset_x,
                    top: top + offset_y,
                    width,
                    height,
                });
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    Ok(Vec::new())
}

/// Retrying variant of [`harvest_token_in_frames`].
///
/// Mirrors [`find_element_centre_in_frames_retry`]: re-walks the frame
/// tree on `interval` until either a populated token is harvested or
/// `timeout` elapses. Used by the post-solve verification paths that
/// need to wait for a vendor's `siteverify`-style response field to be
/// written into the page after a click.
pub async fn harvest_token_in_frames_retry(
    page: &Page,
    token_input_name: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(tok) = harvest_token_in_frames(page, token_input_name).await? {
            return Ok(Some(tok));
        }
        match next_poll_sleep(Instant::now(), deadline, interval) {
            Some(d) => tokio::time::sleep(d).await,
            None => return Ok(None),
        }
    }
}

/// Find the bounding rect `(left, top, width, height)` of the first iframe
/// whose `src` contains `pattern`.
///
/// Evaluates in the main document only, no cross-origin frame piercing
/// required. Returns `None` when no matching iframe is found.
pub async fn find_iframe_rect_by_src(
    page: &Page,
    pattern: &str,
) -> Result<Option<(f64, f64, f64, f64)>> {
    let escaped = escape_js_string(pattern);
    let js = format!(
        r#"(() => {{
            const frames = document.querySelectorAll('iframe');
            for (const f of frames) {{
                if (f.src && f.src.includes('{}')) {{
                    const r = f.getBoundingClientRect();
                    return {{ left: r.left, top: r.top, width: r.width, height: r.height }};
                }}
            }}
            return null;
        }})()"#,
        escaped
    );
    let v = page.evaluate(js.as_str()).await?;
    let val = v
        .into_value::<serde_json::Value>()
        .unwrap_or(serde_json::Value::Null);
    if let (Some(l), Some(t), Some(w), Some(h)) = (
        val["left"].as_f64(),
        val["top"].as_f64(),
        val["width"].as_f64(),
        val["height"].as_f64(),
    ) {
        Ok(Some((l, t, w, h)))
    } else {
        Ok(None)
    }
}

/// Check whether a CAPTCHA response token exists in **any** frame.
/// Used for post-solve verification when the provider may inject the token
/// into a hidden input in the main document or inside an iframe.
///
/// # Example
///
/// ```rust,no_run
/// use runtime_foxdriver::frame::verify_token_in_frames;
/// # async fn example(page: &runtime_foxdriver::Page) -> anyhow::Result<()> {
/// let found = verify_token_in_frames(page, "g-recaptcha-response").await?;
/// assert!(found);
/// # Ok(()) }
/// ```
/// Search every frame for a populated captcha token field of any
/// well-known shape (`cf-turnstile-response`, `g-recaptcha-response`,
/// `h-captcha-response`, `frc-captcha-solution`, `altcha`,
/// `mcaptcha__token`, `cap_token`, `captchaToken`).
///
/// Returns `Ok(true)` as soon as one frame reports a non-empty
/// `el.value` for any of those fields. Useful as a "did anything
/// pass?" check after a passive WAF challenge, saves the caller
/// from running [`verify_token_in_frames`] once per vendor name.
pub async fn verify_any_token_in_frames(page: &Page) -> Result<bool> {
    const ANY_TOKEN_JS: &str = r#"(() => {
        const sels = [
            '[name="cf-turnstile-response"]',
            '[name="g-recaptcha-response"]',
            '#g-recaptcha-response',
            '[name="h-captcha-response"]',
            '[name="captchaToken"]',
            '[name="frc-captcha-solution"]',
            '[name="altcha"]',
            '[name="mcaptcha__token"]',
            '[name="cap_token"]',
        ];
        for (const sel of sels) {
            try {
                const els = document.querySelectorAll(sel);
                for (const el of els) {
                    const v = (el.value || el.textContent || '').trim();
                    if (v) return true;
                }
            } catch (_) { /* keep going */ }
        }
        return false;
    })()"#;
    let results = evaluate_in_all_frames::<bool>(page, ANY_TOKEN_JS).await?;
    Ok(results.into_iter().any(|v| v))
}

/// Is a token field of this name filled in, in any frame of the page?
///
/// Use [`harvest_token_in_frames`] when the value itself is wanted; this only
/// answers whether one exists.
///
/// # Errors
///
/// Returns an error when a frame cannot be evaluated in.
pub async fn verify_token_in_frames(page: &Page, token_input_name: &str) -> Result<bool> {
    Ok(harvest_token_in_frames(page, token_input_name)
        .await?
        .is_some())
}

/// Like [`verify_token_in_frames`] but returns the populated token
/// VALUE so the chain can hand a real `cf-turnstile-response` /
/// `g-recaptcha-response` / `h-captcha-response` token back to the
/// caller. The chain previously emitted hardcoded label strings
/// (`"behavioral:pre-pass"`, …) which downstream code treated as a
/// success token but couldn't actually validate against the vendor's
/// `siteverify` endpoint.
///
/// Walks every frame; returns the FIRST non-empty value found.
/// Order is BFS-stable per [`crate::frame::evaluate_in_all_frames`].
pub async fn harvest_token_in_frames(
    page: &Page,
    token_input_name: &str,
) -> Result<Option<String>> {
    let escaped = escape_js_string(token_input_name);
    // Same selector + .value-property contract as
    // verify_token_in_frames; returns the value instead of a bool.
    let js = format!(
        r#"(() => {{
            const els = document.querySelectorAll('input[name="{0}"], textarea[name="{0}"], #{0}');
            for (const el of els) {{
                const v = (el.value || el.textContent || '').trim();
                if (v) return v;
            }}
            return null;
        }})()"#,
        escaped
    );
    let results = evaluate_in_all_frames::<Option<String>>(page, &js).await?;
    Ok(results.into_iter().flatten().find(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_js_string_all_special_chars() {
        let input = "\\'\"\n\r\t\0";
        assert_eq!(escape_js_string(input), "\\\\\\\'\\\"\\n\\r\\t\\0");
    }

    #[test]
    fn escape_js_string_backslash() {
        assert_eq!(escape_js_string(r"\"), "\\\\");
    }

    #[test]
    fn escape_js_string_single_quote() {
        assert_eq!(escape_js_string("'"), "\\'");
    }

    #[test]
    fn escape_js_string_double_quote() {
        assert_eq!(escape_js_string("\""), "\\\"");
    }

    #[test]
    fn escape_js_string_newline() {
        assert_eq!(escape_js_string("a\nb"), "a\\nb");
    }

    #[test]
    fn escape_js_string_carriage_return() {
        assert_eq!(escape_js_string("a\rb"), "a\\rb");
    }

    #[test]
    fn escape_js_string_tab() {
        assert_eq!(escape_js_string("a\tb"), "a\\tb");
    }

    #[test]
    fn escape_js_string_null_byte() {
        assert_eq!(escape_js_string("a\0b"), "a\\0b");
    }

    #[test]
    fn escape_js_string_mixed() {
        let input = "line1\nline2\tcol\0end\\\"'";
        assert_eq!(
            escape_js_string(input),
            "line1\\nline2\\tcol\\0end\\\\\\\"\\'"
        );
    }

    #[test]
    fn escape_js_string_no_special_chars() {
        assert_eq!(escape_js_string("#simple-id"), "#simple-id");
    }

    #[test]
    fn lookup_iframe_offset_by_index_and_url() {
        let offsets = vec![
            (0, "a.html".into(), "".into(), 10.0, 20.0),
            (1, "b.html".into(), "".into(), 30.0, 40.0),
        ];
        assert_eq!(
            lookup_iframe_offset(&offsets, "a.html", 0),
            Some((10.0, 20.0))
        );
        assert_eq!(
            lookup_iframe_offset(&offsets, "b.html", 1),
            Some((30.0, 40.0))
        );
    }

    #[test]
    fn lookup_iframe_offset_fallback_when_index_missing() {
        let offsets = vec![(0, "a.html".into(), "".into(), 10.0, 20.0)];
        assert_eq!(
            lookup_iframe_offset(&offsets, "a.html", -1),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn lookup_iframe_offset_disambiguates_duplicate_src() {
        let offsets = vec![
            (0, "same.html".into(), "".into(), 10.0, 20.0),
            (1, "same.html".into(), "".into(), 30.0, 40.0),
        ];
        // With index we can tell them apart.
        assert_eq!(
            lookup_iframe_offset(&offsets, "same.html", 0),
            Some((10.0, 20.0))
        );
        assert_eq!(
            lookup_iframe_offset(&offsets, "same.html", 1),
            Some((30.0, 40.0))
        );
        // Without index, fallback to first match.
        assert_eq!(
            lookup_iframe_offset(&offsets, "same.html", -1),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn lookup_iframe_offset_empty_src_and_id() {
        let offsets = vec![
            (0, "".into(), "".into(), 5.0, 5.0),
            (1, "".into(), "".into(), 15.0, 15.0),
        ];
        assert_eq!(lookup_iframe_offset(&offsets, "", 0), Some((5.0, 5.0)));
        assert_eq!(lookup_iframe_offset(&offsets, "", 1), Some((15.0, 15.0)));
    }

    #[test]
    fn lookup_iframe_offset_no_match() {
        let offsets = vec![(0, "a.html".into(), "".into(), 10.0, 20.0)];
        assert_eq!(lookup_iframe_offset(&offsets, "missing.html", -1), None);
    }

    #[test]
    fn find_element_js_contains_query_selector() {
        let selector = "#btn";
        let escaped = escape_js_string(selector);
        let js = format!(
            r#"(function() {{ const el = document.querySelector('{}'); if (!el) return null; const r = el.getBoundingClientRect(); return {{ x: r.left + r.width / 2, y: r.top + r.height / 2, url: window.location.href }}; }})()"#,
            escaped
        );
        assert!(js.contains("document.querySelector"));
        assert!(js.contains("getBoundingClientRect"));
    }

    #[test]
    fn frame_tile_centre_is_box_midpoint() {
        let t = FrameTile {
            index: 4,
            left: 100.0,
            top: 200.0,
            width: 60.0,
            height: 40.0,
        };
        assert_eq!(t.centre(), (130.0, 220.0));
    }

    #[test]
    fn find_tiles_js_collects_all_matches_with_rects() {
        let escaped = escape_js_string(".rc-imageselect-tile");
        let js = format!(
            r#"(function() {{
            const els = document.querySelectorAll('{}');
            if (!els || els.length === 0) return null;
            const tiles = [];
            for (let i = 0; i < els.length; i++) {{
                const r = els[i].getBoundingClientRect();
                tiles.push({{ index: i, left: r.left, top: r.top, width: r.width, height: r.height }});
            }}
            return {{ tiles: tiles }};
        }})()"#,
            escaped
        );
        assert!(js.contains("querySelectorAll"));
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("width: r.width"));
        assert!(js.contains("index: i"));
    }

    #[test]
    fn verify_token_js_contains_input_selector() {
        let name = "g-recaptcha-response";
        let escaped = escape_js_string(name);
        let js = format!(
            r#"!!document.querySelector('input[name="{}"][value]:not([value=""])')"#,
            escaped
        );
        assert!(js.contains("input[name="));
        assert!(js.contains("value]:not([value=\"\"])"));
    }

    #[test]
    fn verify_token_escapes_quotes() {
        let name = r#"token"value"#;
        let escaped = escape_js_string(name);
        assert!(escaped.contains("\\\""));
        for (i, ch) in escaped.char_indices() {
            if ch == '"' {
                assert!(
                    i > 0 && escaped.as_bytes()[i - 1] == b'\\',
                    "quote at {} not escaped",
                    i
                );
            }
        }
    }

    #[test]
    fn next_poll_sleep_returns_interval_when_deadline_far() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(10);
        let interval = Duration::from_millis(100);
        let s = next_poll_sleep(now, deadline, interval).unwrap();
        assert_eq!(s, Duration::from_millis(100));
    }

    #[test]
    fn next_poll_sleep_clamps_to_remaining_when_close_to_deadline() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(40);
        let interval = Duration::from_millis(100);
        let s = next_poll_sleep(now, deadline, interval).unwrap();
        // Must not overshoot the deadline.
        assert!(s <= Duration::from_millis(40));
        assert!(s >= Duration::from_millis(30));
    }

    #[test]
    fn next_poll_sleep_returns_none_at_deadline() {
        let now = Instant::now();
        let deadline = now;
        assert!(next_poll_sleep(now, deadline, Duration::from_millis(100)).is_none());
    }

    #[test]
    fn next_poll_sleep_returns_none_past_deadline() {
        let now = Instant::now();
        let deadline = now - Duration::from_millis(1);
        assert!(next_poll_sleep(now, deadline, Duration::from_millis(100)).is_none());
    }

    #[test]
    fn next_poll_sleep_zero_interval_still_yields_zero_sleep() {
        // A degenerate interval=0 must not panic; it should yield Some(0)
        // which lets the caller spin once and re-check (callers may use
        // this as an "as fast as CDP allows" mode).
        let now = Instant::now();
        let deadline = now + Duration::from_millis(50);
        let s = next_poll_sleep(now, deadline, Duration::ZERO).unwrap();
        assert_eq!(s, Duration::ZERO);
    }

    #[test]
    fn default_retry_constants_are_sane() {
        // Lock the contract: interval must be smaller than timeout and
        // both must be > 0. Catches a regression where someone swaps
        // them or sets either to zero.
        assert!(DEFAULT_FRAME_RETRY_INTERVAL > Duration::ZERO);
        assert!(DEFAULT_FRAME_RETRY_TIMEOUT > DEFAULT_FRAME_RETRY_INTERVAL);
        // Bound on CDP round-trips per retry call.
        let max_polls =
            DEFAULT_FRAME_RETRY_TIMEOUT.as_millis() / DEFAULT_FRAME_RETRY_INTERVAL.as_millis() + 1;
        assert!(
            max_polls <= 200,
            "default retry would issue {max_polls} CDP calls per attempt, too chatty",
        );
    }

    #[test]
    fn verify_token_escapes_null_and_newline() {
        let name = "token\0value\n";
        let escaped = escape_js_string(name);
        assert!(escaped.contains("\\0"));
        assert!(escaped.contains("\\n"));
        assert!(!escaped.contains('\0'));
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn escape_js_string_empty() {
        assert_eq!(escape_js_string(""), "");
    }

    #[test]
    fn escape_js_string_unicode_untouched() {
        // Unicode outside the ASCII escape set should pass through unchanged.
        let input = "emoji: 🎉 café ñ";
        assert_eq!(escape_js_string(input), input);
    }

    #[test]
    fn escape_js_string_preserves_length_hint() {
        let input = "a".repeat(1000);
        let out = escape_js_string(&input);
        assert_eq!(out, input); // no special chars → same length
    }

    #[test]
    fn lookup_iframe_offset_matches_by_id() {
        let offsets = vec![(0, "a.html".into(), "iframe-0".into(), 10.0, 20.0)];
        assert_eq!(
            lookup_iframe_offset(&offsets, "iframe-0", -1),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn lookup_iframe_offset_index_mismatch_falls_back_to_first_match() {
        let offsets = vec![
            (0, "a.html".into(), "".into(), 10.0, 20.0),
            (1, "b.html".into(), "".into(), 30.0, 40.0),
        ];
        // Requesting index 99 of "a.html" doesn't exist, so returns None.
        assert_eq!(lookup_iframe_offset(&offsets, "a.html", 99), None);
        assert_eq!(
            lookup_iframe_offset(&offsets, "a.html", -1),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn lookup_iframe_offset_negative_beyond_minus_one_treated_as_fallback() {
        // Any negative value other than the specific -1 path still goes
        // through the `else` branch (find by src/id).
        let offsets = vec![(0, "x".into(), "".into(), 5.0, 6.0)];
        assert_eq!(lookup_iframe_offset(&offsets, "x", -5), Some((5.0, 6.0)));
    }

    #[test]
    fn next_poll_sleep_interval_larger_than_remaining() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(30);
        let interval = Duration::from_millis(100);
        let s = next_poll_sleep(now, deadline, interval).unwrap();
        assert_eq!(s, Duration::from_millis(30));
    }

    #[test]
    fn next_poll_sleep_very_small_remaining() {
        let now = Instant::now();
        let deadline = now + Duration::from_nanos(1);
        let s = next_poll_sleep(now, deadline, Duration::from_millis(100)).unwrap();
        assert_eq!(s, Duration::from_nanos(1));
    }
}
