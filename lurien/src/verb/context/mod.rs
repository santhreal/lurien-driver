//! Browser context lifecycle: list, create, switch, close.
//! Contexts map to lurien sessions, each with its own profile and cookies.

mod close;
mod create;
mod list;
mod switch;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &list::SPEC,
    &create::SPEC,
    &switch::SPEC,
    &close::SPEC,
];
