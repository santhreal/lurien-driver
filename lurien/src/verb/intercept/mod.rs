//! What happens to a request before it is sent.
//!
//! A route is applied by the engine on the channel, in the parent process: the
//! page cannot see it, cannot refuse it, and a fulfilled request never reaches
//! the network. The table is ordered and the most recently added route is tried
//! first, so a caller narrows behaviour by adding a route.

mod route;
mod route_abort;
mod route_clear;
mod route_continue;
mod route_fulfil;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[
    &route::SPEC,
    &route_abort::SPEC,
    &route_clear::SPEC,
    &route_continue::SPEC,
    &route_fulfil::SPEC,
];
