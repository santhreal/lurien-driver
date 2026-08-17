//! Persona, position, clock, and permissions: what this session claims to be
//! and what it is allowed to do. The persona itself is compiled by `guise`; this
//! domain carries it onto a live session and reports what the browser holds.

mod as_profile;
mod clock;
mod clock_restore;
mod clock_set;
mod clock_tick;
mod geolocation;
mod geolocation_clear;
mod geolocation_set;
mod permissions;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &as_profile::SPEC,
    &clock::SPEC,
    &clock_restore::SPEC,
    &clock_set::SPEC,
    &clock_tick::SPEC,
    &geolocation::SPEC,
    &geolocation_clear::SPEC,
    &geolocation_set::SPEC,
    &permissions::SPEC,
];
