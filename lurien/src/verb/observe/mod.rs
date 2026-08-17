//! Passive page telemetry from the sensor grid: console, uncaught errors, CSP
//! violations, inbound postMessage, and DOM-XSS sink hits.
//!
//! The grid is a preload script installed at launch. `LURIEN_SENSORS=0` runs
//! without it, and these verbs then report that it is absent rather than
//! pretending the page was quiet.

mod console;
mod signals;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
/// Registry entries for the observation domain.
pub static SPECS: &[&VerbSpec] = &[&console::SPEC, &signals::SPEC];

/// Keys the sensor grid reports.
pub(crate) const SIGNAL_KEYS: &[&str] = &[
    "sinks",
    "console",
    "errors",
    "csp",
    "postmessage",
    "counts",
    "installed",
];
