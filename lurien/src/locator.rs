//! One selector language, one wait, for every verb that touches an element.
//!
//! A caller should not have to read the DOM to press a button, and should not
//! have to write a sleep loop because the button arrives a moment after the
//! navigation. Both are the same problem: the driver has to decide what the
//! caller meant and when the page is ready to be acted on. That decision lives
//! here, once, and every act verb goes through it.
//!
//! Forms, resolved in the page by `locator.js`:
//!
//! | Prefix | Meaning |
//! |---|---|
//! | `role:button` / `role:button=Send` | ARIA role, optionally with an accessible name |
//! | `text:Continue` | visible text, substring, innermost element holding it |
//! | `label:Email` | form control whose label or `aria-label` is that text |
//! | `placeholder:you@example.com` | control with that placeholder |
//! | `testid:submit` | `data-testid`, `data-test-id` or `data-test` |
//! | anything else | CSS, unchanged |
//!
//! A semantic form must resolve to exactly one visible, enabled element: a
//! description that fits three buttons is not a description, and clicking the
//! first of them is how an agent presses the wrong one. CSS keeps its
//! first-match contract, because a CSS selector is a precise machine query and
//! callers already depend on that.
//!
//! Resolution returns a CSS path, not a handle, so the act itself goes through
//! the same element path a plain CSS selector does and nothing in the page is
//! mutated to mark the match.

use crate::error::Error;
use runtime_foxdriver::{FrameId, Page};
use std::time::{Duration, Instant};

/// The resolver, evaluated in the page on every attempt. `snapshot.js` is
/// evaluated after it and reuses its role table, names and visibility rules.
pub(crate) const RESOLVER: &str = include_str!("locator.js");

/// How long an act waits for its element by default.
///
/// A page that has not finished laying out is the normal case, not an error, and
/// a caller who wanted to fail fast can say so per call. `LURIEN_TIMEOUT_MS`
/// moves the default for a whole session.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// Gap between resolution attempts. Short enough that a fast page is not slowed
/// by the wait itself.
const POLL_MS: u64 = 100;

/// How the caller described the element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// CSS selector. First match wins.
    Css,
    /// ARIA role, optionally `role=name`.
    Role,
    /// Visible text, matched as a substring.
    Text,
    /// Label or `aria-label` of a form control.
    Label,
    /// Placeholder text.
    Placeholder,
    /// Test id attribute.
    TestId,
}

impl Form {
    /// The token `locator.js` dispatches on.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Css => "css",
            Self::Role => "role",
            Self::Text => "text",
            Self::Label => "label",
            Self::Placeholder => "placeholder",
            Self::TestId => "testid",
        }
    }
}

/// Split a selector into its form and its value.
///
/// An unprefixed selector is CSS, so every selector that worked before this
/// module existed still means what it meant.
#[must_use]
pub fn parse(selector: &str) -> (Form, &str) {
    for (prefix, form) in [
        ("role:", Form::Role),
        ("text:", Form::Text),
        ("label:", Form::Label),
        ("placeholder:", Form::Placeholder),
        ("testid:", Form::TestId),
    ] {
        if let Some(rest) = selector.strip_prefix(prefix) {
            return (form, rest.trim());
        }
    }
    (Form::Css, selector)
}

/// One resolved element.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// CSS path of the element, unique in its document.
    pub css: String,
    /// How the caller described it.
    pub form: Form,
    /// How many elements the description fit.
    pub matched: usize,
    /// How long the wait took.
    pub waited_ms: u64,
}

/// The default deadline for one act, from the environment or the constant.
#[must_use]
pub fn default_timeout_ms() -> u64 {
    std::env::var("LURIEN_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
}

/// Resolve `selector` to one element that is ready to be acted on.
///
/// Polls until the deadline, so an element that arrives after a navigation or a
/// fetch is acted on without an explicit wait. The failure names what was on
/// screen instead, because "no element matched" alone leaves the caller guessing.
pub async fn resolve(page: &Page, selector: &str, timeout_ms: u64) -> Result<Resolved, Error> {
    resolve_with(page, None, selector, timeout_ms, true).await
}

/// Resolve without requiring the element to be actionable.
///
/// A read (`text`, `count`) may legitimately want an element that is present but
/// not clickable; an act may not.
pub async fn resolve_present(
    page: &Page,
    selector: &str,
    timeout_ms: u64,
) -> Result<Resolved, Error> {
    resolve_with(page, None, selector, timeout_ms, false).await
}

/// Resolve inside one browsing context, without requiring actionability.
///
/// A frame-scoped verb reaches its element through the frame's own document, so
/// the same selector language works there: the resolver is plain page script and
/// does not care which document runs it.
pub async fn resolve_present_in(
    page: &Page,
    context: &FrameId,
    selector: &str,
    timeout_ms: u64,
) -> Result<Resolved, Error> {
    resolve_with(page, Some(context), selector, timeout_ms, false).await
}

/// How many elements the description fits, and how many of those are visible.
///
/// Counting does not wait: zero is a legitimate answer to "how many", and a
/// caller asking for a count of a late-arriving list wants the count now.
pub async fn count(page: &Page, selector: &str) -> Result<(usize, usize), Error> {
    let (form, value) = parse(selector);
    let script = format!(
        "(() => {{ {RESOLVER}\n return lurienCount({form}, {value}); }})()",
        form = js_string(form.as_str()),
        value = js_string(value),
    );
    let answer = ask(page, None, &script, selector).await?;
    if answer["ok"].as_bool() != Some(true) {
        return Err(unresolved(selector, &answer, 0));
    }
    let total = answer["matched"].as_u64().unwrap_or(0) as usize;
    let visible = answer["visible"].as_u64().unwrap_or(0) as usize;
    Ok((total, visible))
}

/// One resolver call, parsed. `context` selects the document that runs it; the
/// active one when it is `None`.
async fn ask(
    page: &Page,
    context: Option<&FrameId>,
    script: &str,
    selector: &str,
) -> Result<serde_json::Value, Error> {
    let eval = match context {
        Some(ctx) => page.evaluate_in_context(script.to_string(), ctx).await,
        None => page.evaluate(script.to_string()).await,
    };
    let raw = eval
        .map_err(|e| Error::Other(format!("{selector}: resolver failed: {e}")))?
        .into_value::<String>()
        .map_err(|e| Error::Other(format!("{selector}: resolver returned no answer: {e}")))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::Other(format!("{selector}: resolver answer unreadable: {e}")))
}

async fn resolve_with(
    page: &Page,
    context: Option<&FrameId>,
    selector: &str,
    timeout_ms: u64,
    actionable: bool,
) -> Result<Resolved, Error> {
    let (form, value) = parse(selector);
    let script = format!(
        "(() => {{ {RESOLVER}\n return lurienResolve({form}, {value}, {actionable}); }})()",
        form = js_string(form.as_str()),
        value = js_string(value),
    );
    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms);
    loop {
        let answer = ask(page, context, &script, selector).await?;
        if answer["ok"].as_bool() == Some(true) {
            return Ok(Resolved {
                css: answer["path"].as_str().unwrap_or_default().to_string(),
                form,
                matched: answer["matched"].as_u64().unwrap_or(1) as usize,
                waited_ms: elapsed_ms(started),
            });
        }
        let why = answer["why"].as_str().unwrap_or("none");
        // An unusable selector and an ambiguous description do not improve by
        // waiting: the answer is already final, and holding the caller for the
        // whole deadline would hide it.
        let final_answer = why == "invalid" || why == "ambiguous" || why == "unknown form";
        if final_answer || Instant::now() >= deadline {
            return Err(unresolved(selector, &answer, elapsed_ms(started)));
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS.min(timeout_ms))).await;
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Turn a resolver answer into the error a caller can act on.
fn unresolved(selector: &str, answer: &serde_json::Value, waited_ms: u64) -> Error {
    let matched = answer["matched"].as_u64().unwrap_or(0);
    let detail = match answer["why"].as_str().unwrap_or("none") {
        "invalid" => format!(
            "not a valid selector ({})",
            answer["detail"].as_str().unwrap_or("rejected by the page")
        ),
        "hidden" => format!("{matched} element(s) matched but none is visible"),
        "disabled" => format!("{matched} element(s) matched but none is enabled"),
        "ambiguous" => format!("{matched} visible elements fit that description"),
        "unknown form" => format!(
            "unknown selector form {:?}",
            answer["detail"].as_str().unwrap_or("")
        ),
        _ => "no element matched".to_string(),
    };
    let candidates: Vec<String> = answer["candidates"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let action = if answer["why"].as_str() == Some("ambiguous") {
        format!("narrow it, or use one of: {}", candidates.join("; "))
    } else if candidates.is_empty() {
        "check the selector, or read the page with `snapshot`".to_string()
    } else {
        format!("on screen now: {}", candidates.join("; "))
    };
    Error::Unresolved {
        selector: selector.to_string(),
        detail,
        waited_ms,
        action,
    }
}

/// JSON is a valid JavaScript string literal, so this is the escape.
fn js_string(raw: &str) -> String {
    serde_json::Value::String(raw.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_selector_is_still_css() {
        assert_eq!(parse("#login"), (Form::Css, "#login"));
        assert_eq!(parse("div > button:nth-of-type(2)"), (Form::Css, "div > button:nth-of-type(2)"));
    }

    #[test]
    fn every_form_has_a_prefix_and_keeps_its_value() {
        assert_eq!(parse("role:button=Send"), (Form::Role, "button=Send"));
        assert_eq!(parse("text: Continue "), (Form::Text, "Continue"));
        assert_eq!(parse("label:Email"), (Form::Label, "Email"));
        assert_eq!(parse("placeholder:you@example.com"), (Form::Placeholder, "you@example.com"));
        assert_eq!(parse("testid:submit"), (Form::TestId, "submit"));
    }

    /// A CSS selector containing a colon must not be read as a form, or every
    /// pseudo-class becomes an unknown form.
    #[test]
    fn a_pseudo_class_is_not_a_form() {
        for selector in ["a:hover", "input:checked", "li:nth-child(2)", "svg|circle"] {
            assert_eq!(parse(selector).0, Form::Css, "{selector} was read as a form");
        }
    }

    /// The resolver is one file evaluated as one expression. A syntax error in it
    /// would surface as every selector failing at run time, so the shape of the
    /// call is pinned here.
    #[test]
    fn the_resolver_is_called_with_a_json_string_form_and_value() {
        let script = format!(
            "(() => {{ {RESOLVER}\n return lurienResolve({}, {}, {}); }})()",
            js_string("role"),
            js_string("button=Log in"),
            true
        );
        assert!(script.contains("function lurienResolve(form, value, need)"));
        assert!(script.contains(r#"lurienResolve("role", "button=Log in", true)"#));
        assert!(!RESOLVER.contains("document.write"));
    }

    #[test]
    fn the_default_deadline_comes_from_the_environment_or_the_constant() {
        // The env var is process-wide, so this test owns it and restores nothing:
        // an unset variable is the default, which is what the constant is for.
        assert_eq!(DEFAULT_TIMEOUT_MS, 10_000);
        assert!(default_timeout_ms() >= 1);
    }

    #[test]
    fn a_failure_names_the_wait_and_what_was_on_screen() {
        let answer = serde_json::json!({
            "ok": false,
            "why": "none",
            "candidates": ["button \"Send\"", "link \"Home\""],
        });
        let err = unresolved("role:button=Submit", &answer, 4_200);
        let text = err.to_string();
        assert!(text.contains("role:button=Submit"), "{text}");
        assert!(text.contains("no element matched"), "{text}");
        assert!(text.contains("4200ms"), "{text}");
        assert!(text.contains("button \"Send\""), "{text}");
    }

    #[test]
    fn an_ambiguous_description_is_refused_with_the_candidates() {
        let answer = serde_json::json!({
            "ok": false,
            "why": "ambiguous",
            "matched": 3,
            "candidates": ["form > button \"Send\"", "footer > button \"Send\""],
        });
        let text = unresolved("text:Send", &answer, 12).to_string();
        assert!(text.contains("3 visible elements"), "{text}");
        assert!(text.contains("narrow it"), "{text}");
    }
}
