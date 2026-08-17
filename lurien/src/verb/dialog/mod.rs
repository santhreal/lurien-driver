//! JavaScript dialogs and downloads. Capture is armed at launch, so an
//! `alert()` that fired during navigation is still evidence afterwards.

mod answer;
mod clear;
mod download_save;
mod download_wait;
mod downloads;
mod list;

use crate::verb::VerbSpec;

/// Registry entries for the dialog domain. A new verb is one line here plus its
/// own file.
pub static SPECS: &[&VerbSpec] = &[
    &answer::SPEC,
    &clear::SPEC,
    &download_save::SPEC,
    &download_wait::SPEC,
    &downloads::SPEC,
    &list::SPEC,
];
