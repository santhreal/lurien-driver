//! Browsing-context tree. Cross-origin frames are first-class here: a captcha
//! widget is an OOPIF, and page JavaScript cannot reach it.

mod click_in;
mod eval;
mod list;
mod tree;
mod type_in;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &click_in::SPEC,
    &eval::SPEC,
    &list::SPEC,
    &tree::SPEC,
    &type_in::SPEC,
];
