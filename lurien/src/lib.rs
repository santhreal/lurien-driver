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
pub mod error;
pub mod goto;
pub mod launch;
pub mod mcp;
pub mod profile_import;
pub mod resolve;
pub mod serve;
pub mod session;
pub mod verb;
pub mod version;

use guise::StealthProfile;
use launch::LaunchOptions;
use runtime_foxdriver::{CapturedCookie, FrameId, Page};

pub use as_profile::as_profile;
pub use error::Error;
pub use challenge::{ChallengeConfig, EngineOutcome};
pub use goto::{ChallengeKind, GotoOutcome};
pub use launch::LaunchOptions as BrowserLaunchOptions;
pub use profile_import::{import_profile, ImportReport};
pub use resolve::{resolve_engine, resolve_engine_checked};
pub use session::Session;
pub use verb::{Args, Output, VerbSpec};
pub use version::{crate_version, engine_version_string, version_line};

/// Public face. Wraps a lurien-engine [`Page`].
pub struct Browser {
    page: Page,
}

impl Browser {
    /// Launch wearing `profile`. Engine required. Default headful.
    pub async fn launch(profile: StealthProfile) -> Result<Self, Error> {
        Ok(Self {
            page: launch::launch(profile).await?,
        })
    }

    /// Launch with explicit options.
    pub async fn launch_with_options(opts: LaunchOptions) -> Result<Self, Error> {
        Ok(Self {
            page: launch::launch_with_options(opts).await?,
        })
    }

    /// Import a real Firefox profile, then launch wearing it.
    pub async fn as_profile(
        src: &std::path::Path,
        dest: Option<&std::path::Path>,
        profile: StealthProfile,
        headless: bool,
    ) -> Result<(Self, ImportReport), Error> {
        let (page, report) = as_profile::as_profile(src, dest, profile, headless, None).await?;
        Ok((Self { page }, report))
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

    /// Click the first match of `selector`.
    pub async fn click(&self, selector: &str) -> Result<(), Error> {
        let el = self
            .page
            .find_element(selector)
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

    /// Focus `selector` and type `text`.
    pub async fn fill(&self, selector: &str, text: &str) -> Result<(), Error> {
        let el = self
            .page
            .find_element(selector)
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

    /// Title + URL + a short text snapshot (Playwright-MCP `snapshot`).
    pub async fn snapshot(&self) -> Result<String, Error> {
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
