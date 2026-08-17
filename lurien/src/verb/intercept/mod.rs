//! Request/response interception and header manipulation.
//! BiDi network interception is used where available; header manipulation
//! falls back to eval-based overrides for request headers.

mod clear;
mod delete_header;
mod get_headers;
mod intercept_request;
mod intercept_response;
mod set_header;
mod set_extra_headers;

use crate::verb::VerbSpec;

/// Registry entries for the interception domain.
pub static SPECS: &[&VerbSpec] = &[
    &get_headers::SPEC,
    &set_header::SPEC,
    &delete_header::SPEC,
    &set_extra_headers::SPEC,
    &intercept_request::SPEC,
    &intercept_response::SPEC,
    &clear::SPEC,
];
