//! Session-level verbs: work that is about the sequence of calls rather than
//! about the page.

pub mod batch;

use super::VerbSpec;

/// Registered specs for this domain.
pub static SPECS: &[&VerbSpec] = &[&batch::SPEC];
