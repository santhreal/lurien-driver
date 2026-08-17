//! Whole-origin state: cookies plus local and session storage, as one snapshot
//! that round-trips.
//!
//! `state` reads it, `state-set` restores it, `state-clear` drops storage,
//! service workers, and caches. The snapshot shape is versioned so a stale blob
//! is refused instead of silently half-applied.

mod clear;
mod get;
mod set;

use crate::verb::VerbSpec;

/// Verbs of this domain. A new verb is one line here plus its own file.
/// Registry entries for the session-state domain.
pub static SPECS: &[&VerbSpec] = &[&clear::SPEC, &get::SPEC, &set::SPEC];

/// Snapshot format version. Bump when the shape changes; `state-set` refuses
/// anything it does not recognize.
pub(crate) const SNAPSHOT_VERSION: u32 = 1;

/// Read local and session storage as `{ "local": {...}, "session": {...} }`.
pub(crate) const READ_STORAGE_JS: &str = r#"(() => {
    const dump = (s) => {
        const out = {};
        try {
            for (let i = 0; i < s.length; i++) {
                const k = s.key(i);
                out[k] = s.getItem(k);
            }
        } catch (e) { /* opaque origin */ }
        return out;
    };
    return JSON.stringify({ local: dump(localStorage), session: dump(sessionStorage) });
})()"#;
