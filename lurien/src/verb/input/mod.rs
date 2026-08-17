//! Raw trusted input. Trajectories come from `guise::human`, never from a
//! native model inside the engine.

mod mouse_move;
mod press;
mod scroll;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
pub static SPECS: &[&VerbSpec] = &[&mouse_move::SPEC, &press::SPEC, &scroll::SPEC];
