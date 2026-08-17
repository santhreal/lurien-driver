//! Element-scoped work. Every verb here takes a selector or its text.

mod click;
mod count;
mod fill;
mod select;
mod text;
mod type_text;
mod upload;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &click::SPEC,
    &count::SPEC,
    &fill::SPEC,
    &select::SPEC,
    &text::SPEC,
    &type_text::SPEC,
    &upload::SPEC,
];
