//! lurien: a Firefox you drive like Playwright.
//!
//! Engine binary required. Missing binary is an error. There is no
//! `/usr/bin/firefox` fallback and no `challenge` tool.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::panic
    )
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc
)]

pub mod as_profile;
pub mod catalog;
pub mod challenge;
pub mod chooser;
pub mod clock;
pub mod control;
pub mod download;
pub mod error;
pub mod geo;
pub mod goto;
pub mod launch;
pub mod locator;
pub mod mcp;
pub mod permission;
pub mod profile_import;
pub mod resolve;
pub mod route;
pub mod serve;
pub mod shot;
pub mod session;
pub mod snapshot;
pub mod verb;
pub mod version;

use guise::StealthProfile;
use launch::LaunchOptions;
use runtime_foxdriver::{CapturedCookie, FrameId, Page};

pub use as_profile::as_profile;
pub use error::Error;
pub use challenge::{ChallengeConfig, EngineOutcome};
pub use clock::Reading as ClockReading;
pub use control::Control;
pub use geo::{Geolocation, Position};
pub use goto::{ChallengeKind, GotoOutcome};
pub use launch::LaunchOptions as BrowserLaunchOptions;
pub use permission::{Grant, PermissionPolicy};
pub use locator::{Form, Resolved};
pub use snapshot::{Node, Snapshot};
pub use profile_import::{import_profile, ImportReport};
pub use resolve::{resolve_engine, resolve_engine_checked};
pub use session::Session;
pub use verb::{Args, Output, VerbSpec};
pub use version::{crate_version, engine_version_string, version_line};

/// Public face. Wraps a lurien-engine [`Page`].
pub struct Browser {
    page: Page,
    /// Handles from the last snapshot. A handle is a promise about one node, so
    /// the table lives here rather than in the page: tagging the DOM to mark a
    /// match would be visible to page script.
    handles: std::sync::Mutex<Option<Snapshot>>,
    /// The position this session serves and the channel that moves it. Held here
    /// because the engine was told about the channel at launch.
    geo: std::sync::Arc<geo::Geolocation>,
}

impl Browser {
    /// Launch wearing `profile`. Engine required. Default headful.
    pub async fn launch(profile: StealthProfile) -> Result<Self, Error> {
        Ok(Self::wrap(launch::launch(profile).await?))
    }

    /// Launch with explicit options.
    pub async fn launch_with_options(opts: LaunchOptions) -> Result<Self, Error> {
        Ok(Self::wrap(launch::launch_with_options(opts).await?))
    }

    /// Import a real Firefox profile, then launch wearing it.
    ///
    /// Takes the whole launch contract, so an imported profile keeps the
    /// session's permissions and position service instead of quietly launching
    /// with the defaults.
    pub async fn as_profile(
        src: &std::path::Path,
        dest: Option<&std::path::Path>,
        opts: LaunchOptions,
    ) -> Result<(Self, ImportReport), Error> {
        let (launched, report) = as_profile::as_profile(src, dest, opts).await?;
        Ok((Self::wrap(launched), report))
    }

    /// The position this session serves and the channel that moves it.
    #[must_use]
    pub fn geo(&self) -> &std::sync::Arc<geo::Geolocation> {
        &self.geo
    }

    fn wrap(launched: launch::Launched) -> Self {
        Self {
            page: launched.page,
            handles: std::sync::Mutex::new(None),
            geo: launched.geo,
        }
    }

    /// Navigate. Captcha is classified here. No `auto_solve`.
    pub async fn goto(&self, url: &str) -> Result<GotoOutcome, Error> {
        goto::goto(&self.page, url).await
    }

    /// Current URL.
    pub async fn url(&self) -> Result<String, Error> {
        self.page
            .url()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Viewport PNG bytes.
    pub async fn screenshot(&self) -> Result<Vec<u8>, Error> {
        self.page
            .screenshot()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// All cookies, including HttpOnly.
    pub async fn cookies(&self) -> Result<Vec<CapturedCookie>, Error> {
        self.page
            .get_cookies()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Resolve `selector`, waiting for an element ready to be acted on.
    ///
    /// Every verb that touches an element goes through this, so `role:`, `text:`,
    /// `label:`, `placeholder:`, `testid:` and a snapshot handle (`ref:e3`) work
    /// everywhere a CSS selector does and an element that arrives late is waited
    /// for rather than missed.
    pub async fn locate(&self, selector: &str, timeout_ms: u64) -> Result<Resolved, Error> {
        let selector = self.deref_handle(selector).await?;
        locator::resolve(&self.page, &selector, timeout_ms).await
    }

    /// Resolve `selector` for a read: present is enough, visible is not required.
    pub async fn locate_present(
        &self,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<Resolved, Error> {
        let selector = self.deref_handle(selector).await?;
        locator::resolve_present(&self.page, &selector, timeout_ms).await
    }

    /// Turn a snapshot handle into the CSS path it stands for, once its node is
    /// confirmed to still be the node the handle was captured for.
    ///
    /// Anything that is not a handle is returned untouched, so this costs a
    /// prefix check for every other selector.
    async fn deref_handle(&self, selector: &str) -> Result<String, Error> {
        let Some(handle) = selector.strip_prefix("ref:") else {
            return Ok(selector.to_string());
        };
        let known = {
            let table = self.handles.lock().map_err(|_| poisoned())?;
            match table.as_ref() {
                None => {
                    return Err(Error::Unresolved {
                        selector: selector.to_string(),
                        detail: "no snapshot has been taken in this session".to_string(),
                        waited_ms: 0,
                        action: "call `snapshot` first, then use a handle it reports".to_string(),
                    })
                }
                Some(snap) => snap.node(handle).cloned().ok_or_else(|| Error::Unresolved {
                    selector: selector.to_string(),
                    detail: format!("no such handle in the last snapshot ({})", snap.handles()),
                    waited_ms: 0,
                    action: "take a fresh snapshot and use the handle it reports".to_string(),
                })?,
            }
        };
        snapshot::verify(&self.page, &known).await?;
        Ok(known.path)
    }

    /// Click the first match of `selector`, waiting for it to be actionable.
    pub async fn click(&self, selector: &str) -> Result<(), Error> {
        self.click_within(selector, locator::default_timeout_ms()).await
    }

    /// Click, with the caller's own deadline.
    pub async fn click_within(&self, selector: &str, timeout_ms: u64) -> Result<(), Error> {
        let found = self.locate(selector, timeout_ms).await?;
        let el = self
            .page
            .find_element(&found.css)
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        el.click().await.map_err(|e| Error::Other(e.to_string()))
    }

    /// Type into the focused element.
    pub async fn type_text(&self, text: &str) -> Result<(), Error> {
        self.page
            .type_text(text)
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Focus `selector` and type `text`, waiting for the field to be actionable.
    pub async fn fill(&self, selector: &str, text: &str) -> Result<(), Error> {
        self.fill_within(selector, text, locator::default_timeout_ms()).await
    }

    /// Fill, with the caller's own deadline.
    pub async fn fill_within(
        &self,
        selector: &str,
        text: &str,
        timeout_ms: u64,
    ) -> Result<(), Error> {
        let found = self.locate(selector, timeout_ms).await?;
        let el = self
            .page
            .find_element(&found.css)
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        el.click().await.map_err(|e| Error::Other(e.to_string()))?;
        el.type_text(text)
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Wheel scroll at the current mouse position.
    pub async fn scroll(&self, dx: i64, dy: i64) -> Result<(), Error> {
        self.page
            .scroll(dx, dy)
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Sleep. Bound is the caller's.
    pub async fn wait(&self, ms: u64) -> Result<(), Error> {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        Ok(())
    }

    /// Browsing-context ids (main + iframes).
    pub async fn frames(&self) -> Result<Vec<FrameId>, Error> {
        self.page
            .frames()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// The page as an addressable node list, and the handle table behind it.
    ///
    /// This is the representation an agent should act from: roles, names, states
    /// and one handle per node. Every handle stays usable as `ref:eN` until the
    /// next snapshot replaces the table or the node it names changes.
    pub async fn snapshot(&self, limit: usize) -> Result<Snapshot, Error> {
        let snap = snapshot::capture(&self.page, limit).await?;
        *self.handles.lock().map_err(|_| poisoned())? = Some(snap.clone());
        Ok(snap)
    }

    /// Title, URL and the page's visible text, for when the node list is not
    /// what the caller is after.
    pub async fn snapshot_text(&self) -> Result<String, Error> {
        let title = self
            .page
            .title()
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        let url = self.url().await?;
        let body = self
            .page
            .evaluate("(document.body && document.body.innerText || '').slice(0, 4000)")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default();
        Ok(format!("title: {title}\nurl: {url}\n\n{body}"))
    }

    /// The document's serialized markup, on request.
    pub async fn source(&self) -> Result<String, Error> {
        self.page
            .evaluate("document.documentElement.outerHTML")
            .await
            .map_err(|e| Error::Other(e.to_string()))?
            .into_value()
            .map_err(|e| Error::Other(e.to_string()))
    }

    /// Borrow the underlying BiDi page.
    #[must_use]
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// Graceful close (flushes profile storage).
    pub async fn close(self) -> Result<(), Error> {
        self.page
            .close()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }
}

/// A poisoned handle table means a panic already happened elsewhere. Reporting it
/// is honest; unwrapping would turn one panic into two.
fn poisoned() -> Error {
    Error::Other("the handle table was left poisoned by an earlier panic".to_string())
}
