//! JavaScript dialogs and downloads. Capture is armed at launch, so an
//! `alert()` that fired during navigation is still evidence afterwards.

mod answer;
mod clear;
mod list;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
/// Registry entries for the dialog domain.
pub static SPECS: &[&VerbSpec] = &[&answer::SPEC, &clear::SPEC, &list::SPEC];
