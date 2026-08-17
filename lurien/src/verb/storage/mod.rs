//! Cookies and web storage. Reads go through BiDi storage, not page JavaScript,
//! so HttpOnly is visible.

mod clear_cookies;
mod cookies;
mod delete_cookie;
mod set_cookie;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &cookies::SPEC,
    &set_cookie::SPEC,
    &delete_cookie::SPEC,
    &clear_cookies::SPEC,
];
