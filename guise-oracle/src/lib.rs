//! `guise-oracle` - shared surface taxonomy and differential oracle data types
//!
//! [![santh status](https://img.shields.io/badge/santh-beta-blue)](https://santh.dev/standard)
//!
//! This crate publishes the data-shaped contract that the differential oracle,
//! the fingerprint inventory, and downstream stealth consumers (`lurien`, `guise`,
//! `sear`) agree on: [`Severity`], [`Probe`], [`ProbeOutcome`],
//! [`Capture`], [`DifferentialReport`], and [`ThreeWayReport`]. It contains zero
//! browser-driver, TLS, or heavy dependencies so it can be imported cheaply across
//! any tier of the stealth stack.
//!
//! Runtime evaluation, rendering, and scorecard generation live in the `guise`
//! crate, which re-exports these types from its `probe` module.
//!
//! ## Quick Start
//!
//! ```rust
//! use guise_oracle::{Capture, CapturedSurface, Severity};
//!
//! // Create offline captures from two browser profiles
//! let stock = Capture {
//!     label: "stock-firefox-135".into(),
//!     surfaces: vec![CapturedSurface {
//!         name: "navigator.webdriver".into(),
//!         severity: Severity::High,
//!         value: Ok("false".into()),
//!     }],
//! };
//!
//! let target = Capture {
//!     label: "patched-lurien-135".into(),
//!     surfaces: vec![CapturedSurface {
//!         name: "navigator.webdriver".into(),
//!         severity: Severity::High,
//!         value: Ok("false".into()),
//!     }],
//! };
//!
//! // Perform offline differential analysis without browser drivers
//! let report = stock.diff(&target);
//! assert!(report.is_identical());
//! assert_eq!(report.agreed, 1);
//! ```
//!
//! ## When to use / when not to use
//!
//! - **Use when:**
//!   - You are building fingerprint probes, report renderers, or differential diffing tooling.
//!   - You need serializable JSON report schemas shared between automation analyzers and JS engines.
//!   - You want offline differential comparison between captured browser surface fixtures.
//! - **Do not use when:**
//!   - You need live browser orchestration or CDP automation (use `guise` or `lurien` instead).
//!   - You require full browser profile generator state (use `guise-profiles` instead).
//!
//! ## Compared to alternatives
//!
//! Traditional browser bot-detection frameworks tightly couple surface taxonomy with
//! browser automation runtimes, requiring headless browsers or heavy Node.js dependencies
//! just to parse or diff report formats. `guise-oracle` decouples the taxonomy contract
//! into a lightweight Rust crate that serializes deterministically and diffs offline in sub-millisecond time.
//!
//! Unlike unstructured raw JSON diffing tools, `guise-oracle` classifies divergences by
//! severity (High, Medium, Low) and isolates persona-intended overrides from unexpected engine-level leaks.
//!
//! ## How it fits in Santh
//!
//! `guise-oracle` lives under `software/browser/` in the Santh monorepo. It serves as the
//! shared taxonomy contract between the lurien engine, high-level stealth
//! drivers (`guise`), and verification engines (`sear`).

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
mod types;

pub use types::{
    severity_rank, Capture, CapturedSurface, Determinism, DifferentialReport, Divergence,
    DivergenceKind, DriftReport, Probe, ProbeOutcome, ProbeReport, Severity, ThreeWayReport,
    ThreeWaySurface,
};
