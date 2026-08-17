//! The file chooser a page opens from script.
//!
//! `upload` works when the caller can name the file input. Plenty of pages
//! cannot be driven that way: the input is hidden behind a styled button, and the
//! page calls `input.click()` in its own handler. That click opens the OS file
//! picker, which in an unattended session is a dialog nobody will ever answer, so
//! the page stalls and the run is lost.
//!
//! So the chooser is armed before the trigger is pressed: the shim cancels the
//! default action of the click that would open the picker, remembers which input
//! it was, and the driver attaches the caller's files to that input. The page's
//! own listeners still run, and nothing is intercepted unless a caller asked.

use crate::error::Error;
use runtime_foxdriver::Page;

/// The shim, evaluated in the page after the resolver whose `lurienPath` it uses.
const SHIM: &str = include_str!("chooser.js");

/// Gap between checks for a caught chooser.
const POLL_MS: u64 = 50;

/// An input whose chooser was intercepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caught {
    /// CSS path of the file input.
    pub path: String,
    /// Name, id, or a fallback, for the reply a caller reads.
    pub tag: String,
}

/// Arm the page to catch the next file chooser it opens.
pub async fn arm(page: &Page) -> Result<(), Error> {
    let script = format!(
        "(() => {{ {resolver}\n{shim}\n return lurienArmChooser(); }})()",
        resolver = crate::locator::RESOLVER,
        shim = SHIM,
    );
    let answer: String = page
        .evaluate(&script)
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .into_value()
        .map_err(|e| Error::Other(format!("arming the file chooser failed: {e}")))?;
    if answer == "armed" {
        return Ok(());
    }
    Err(Error::Other(format!(
        "arming the file chooser answered {answer:?}"
    )))
}

/// The input whose chooser has been caught, if one has been yet.
pub async fn caught(page: &Page) -> Result<Option<Caught>, Error> {
    let script = format!(
        "(() => {{ {resolver}\n{shim}\n return lurienCaughtChooser(); }})()",
        resolver = crate::locator::RESOLVER,
        shim = SHIM,
    );
    let answer: String = page
        .evaluate(&script)
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .into_value()
        .map_err(|e| Error::Other(format!("the file chooser did not answer: {e}")))?;
    parse(&answer)
}

/// Wait for the page to open a chooser, up to `timeout_ms`.
pub async fn wait(page: &Page, timeout_ms: u64) -> Result<Caught, Error> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        if let Some(found) = caught(page).await? {
            return Ok(found);
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::Unresolved {
                selector: "file chooser".to_string(),
                detail: "the trigger was pressed but no file chooser opened".to_string(),
                waited_ms: timeout_ms,
                action: "Check that the trigger opens a file input, or use `upload` with the \
                         input's own selector."
                    .to_string(),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
    }
}

fn parse(answer: &str) -> Result<Option<Caught>, Error> {
    let value: serde_json::Value =
        serde_json::from_str(answer).map_err(|e| Error::Other(e.to_string()))?;
    if value["ok"].as_bool() != Some(true) {
        return Ok(None);
    }
    let path = value["path"].as_str().unwrap_or_default().to_string();
    if path.is_empty() {
        return Ok(None);
    }
    Ok(Some(Caught {
        path,
        tag: value["tag"].as_str().unwrap_or("file input").to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caught chooser with no address is not a catch: attaching files to an
    /// empty selector would find the first element on the page.
    #[test]
    fn an_answer_without_a_path_is_not_a_catch() {
        assert_eq!(parse(r#"{"ok":false}"#).expect("parses"), None);
        assert_eq!(
            parse(r#"{"ok":true,"path":"","tag":"x"}"#).expect("parses"),
            None
        );
    }

    #[test]
    fn a_catch_carries_the_input_and_what_to_call_it() {
        let caught = parse(r#"{"ok":true,"path":"form > input:nth-of-type(2)","tag":"resume"}"#)
            .expect("parses")
            .expect("a catch");
        assert_eq!(caught.path, "form > input:nth-of-type(2)");
        assert_eq!(caught.tag, "resume");
    }

    /// The shim must not intercept a chooser nobody asked about, and must catch
    /// exactly one: a page that opens pickers in a loop would otherwise have every
    /// one of them cancelled for the rest of the session.
    #[test]
    fn the_shim_is_armed_for_one_chooser_and_cancels_only_the_default_action() {
        assert!(SHIM.contains("if (!state.armed)"), "unarmed clicks pass through");
        assert!(SHIM.contains("state.armed = false;"), "a catch disarms");
        assert!(SHIM.contains("event.preventDefault();"));
        assert!(
            !SHIM.contains("stopPropagation") && !SHIM.contains("stopImmediatePropagation"),
            "the page's own listeners must still see the click"
        );
    }
}
