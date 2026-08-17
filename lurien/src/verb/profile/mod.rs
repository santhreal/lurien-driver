//! Persona, position, and permissions: what this session claims to be and what
//! it is allowed to do. The persona itself is compiled by `guise`; this domain
//! carries it onto a live session and reports what the browser holds.

mod as_profile;
mod geolocation;
mod geolocation_clear;
mod geolocation_set;
mod permissions;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &as_profile::SPEC,
    &geolocation::SPEC,
    &geolocation_clear::SPEC,
    &geolocation_set::SPEC,
    &permissions::SPEC,
];
