//! Browser launch via rustenium Firefox (replaces runtime_headless).
//!
//! CLI, examples, and bindings should use [`drive_browser`] instead of
//! hand-rolling `captchaforge::browser::launch_firefox` + handler tasks.

use anyhow::{Context, Result};

use crate::browser::{launch_firefox, FoxBrowserConfig, Page};

/// Options for [`drive_browser`].
#[derive(Debug, Clone)]
pub struct BrowserDriveOptions {
    /// When false, launch a visible window.
    pub headless: bool,
    /// Disable chromium sandbox (no-op for Firefox, kept for API compat).
    pub no_sandbox: bool,
}

impl Default for BrowserDriveOptions {
    fn default() -> Self {
        Self {
            headless: true,
            no_sandbox: true,
        }
    }
}

/// Build launch config aligned with the old runtime-headless shape.
#[must_use]
pub fn launch_options(opts: &BrowserDriveOptions) -> FoxBrowserConfig {
    FoxBrowserConfig {
        headless: opts.headless,
        ..Default::default()
    }
}

/// Launch Firefox, navigate to `url`, run `f` with the live page, then tear down.
pub async fn drive_browser<F, Fut, T>(url: &str, opts: BrowserDriveOptions, f: F) -> Result<T>
where
    F: FnOnce(Page) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let page = launch_firefox(launch_options(&opts))
        .await
        .map_err(|e| anyhow::anyhow!("launch firefox (is it installed and on PATH?): {e}"))?;
    tokio::time::timeout(std::time::Duration::from_secs(30), page.goto(url))
        .await
        .map_err(|_| anyhow::anyhow!("navigate to {url} timed out after 30s"))?
        .with_context(|| format!("navigate to {url}"))?;
    f(page).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_options_headless_maps_correctly() {
        let opts = launch_options(&BrowserDriveOptions {
            headless: true,
            ..BrowserDriveOptions::default()
        });
        assert!(opts.headless);
    }

    #[test]
    fn launch_options_headful_maps_correctly() {
        let opts = launch_options(&BrowserDriveOptions {
            headless: false,
            ..BrowserDriveOptions::default()
        });
        assert!(!opts.headless);
    }
}
