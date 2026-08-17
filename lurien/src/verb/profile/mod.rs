//! Persona and real-profile import. The persona itself is compiled by `guise`;
//! this domain only carries it onto a live session.

mod as_profile;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[&as_profile::SPEC];
