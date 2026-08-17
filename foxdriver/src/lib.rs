//! # runtime-foxdriver
//!
//! [![santh status](https://img.shields.io/badge/santh-beta-blue)](https://santh.dev/standard)
//!
//! Firefox browser automation via WebDriver BiDi (`rustenium`).
//!
//! This crate provides a spawn-capable Firefox runtime: launch, drive, evaluate,
//! click, type, scroll, screenshot, cookies, dialogs, and cross-origin frame graphs.
//! It is intentionally **independent** of `guise` (the stealth substrate) so the fleet's
//! layering stays one-way: guise may depend on `foxdriver` for its browser feature,
//! but `foxdriver` knows nothing about stealth profiles, fingerprint bundles, or TLS impersonation.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use runtime_foxdriver::{drive_browser, BrowserDriveOptions};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     drive_browser("https://example.com", BrowserDriveOptions::default(), |page| async move {
//!         let res = page.evaluate("document.title").await?;
//!         let title: String = res.into_value()?;
//!         println!("Page title: {title}");
//!         Ok(())
//!     }).await
//! }
//! ```
//!
//! ## When to use / when not to use
//!
//! ### When to use
//! - Driving Firefox browsers natively via WebDriver BiDi for automation or scanning.
//! - Capturing passive network traffic, JS dialogs, page downloads, and DOM security signals.
//! - Traversing cross-origin iframe hierarchies and translating frame coordinates to main viewport space.
//!
//! ### When not to use
//! - You need stealth/fingerprint spoofing directly: use `guise::browser` (which wraps `foxdriver` with stealth profiles).
//! - You need Chromium / Playwright / CDP-specific driver bindings: use the corresponding runtime driver crate.
//!
//! ## Compared to alternatives
//!
//! Unlike raw selenium or marionette drivers, `runtime-foxdriver` uses WebDriver BiDi event streams for async, non-blocking telemetry and robust readiness-polled browser launches.
//!
//! Compared to headless chromium drivers, Firefox via BiDi provides native gecko rendering, full cross-origin OOPIF frame graph traversal, and clean SIGTERM graceful profile persistence before shutdown.
//!
//! ## How it fits in Santh
//!
//! `runtime-foxdriver` lives in `libs/runtime` as the primary Firefox browser driver primitive in Santh. Higher-level automation tools and stealth engines (such as `guise`) build on top of `runtime-foxdriver`.
//!
//! ## License
//!
//! MIT OR Apache-2.0

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
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

pub mod browser;
pub mod cookies;
pub mod dialog;
pub mod frame;
pub mod frame_graph;
pub mod network;
pub mod runtime;
pub mod sensors;

// Re-export the most common types at the crate root for ergonomics.
pub use browser::{
    launch_firefox, launch_firefox_self_managed, proxy_prefs, Element, EvaluationResult,
    FoxBrowserConfig, FrameId, FrameInfo, FrameTreeNode, Page, ProxyConfig, ProxyScheme,
    ScrollDirection,
};
pub use cookies::CapturedCookie;
pub use dialog::{CapturedDialog, CapturedDownload, DialogLog};
pub use frame_graph::{FrameGraph, FrameNode};
pub use network::{
    CapturedHeader, CapturedRequest, CapturedResponse, Filter, NetworkEntry, NetworkLog,
};
pub use runtime::{drive_browser, BrowserDriveOptions};
