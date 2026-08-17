//! Names for frames that do not move under the caller.
//!
//! A browsing context can be addressed by its own id, by its position in the
//! frame list, or by a piece of its URL. Two of those three change while a run is
//! in progress: an index shifts when a frame attaches or detaches, and a URL
//! changes on every navigation. Both fail the same way, which is the dangerous
//! one: they resolve to a different document rather than to nothing.
//!
//! A handle is minted the first time a context is seen and never reused. It maps
//! to the context id, which Gecko keeps across a navigation, so `f2` is the same
//! frame before and after it reloads. When the context is gone the handle is
//! refused and says what it used to be, because acting on the wrong frame is
//! worse than not acting.

use crate::error::Error;

/// One frame this session has named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The name a caller uses: `f1`, `f2`, and so on.
    pub handle: String,
    /// The browsing context it was minted for.
    pub context: String,
    /// Where that context was when it was last seen.
    pub url: String,
    /// `window.name`, when it has one.
    pub name: String,
    /// Whether the context was in the tree the last time it was read.
    pub live: bool,
}

/// Every frame this session has named, in the order they were first seen.
#[derive(Debug, Default)]
pub struct Handles {
    slots: Vec<Slot>,
}

impl Handles {
    /// Take the live frame tree and name anything new in it.
    ///
    /// The input is the tree the browser reports right now, `(context, url)` per
    /// context, and nothing else: a cached list of contexts would keep a frame
    /// that has been removed looking alive, which is exactly the answer a handle
    /// exists to refuse.
    ///
    /// A context that has gone is kept, marked dead: a caller holding its handle
    /// gets an answer about that frame rather than another frame's document.
    pub fn refresh(&mut self, live: &[(String, String)]) {
        for slot in &mut self.slots {
            slot.live = false;
        }
        for (context, url) in live {
            match self.slots.iter_mut().find(|slot| &slot.context == context) {
                Some(slot) => {
                    slot.url = url.clone();
                    slot.live = true;
                }
                None => {
                    // Handles count up for the life of the session and are never
                    // reused, so a stale handle can always be told from a new one.
                    let handle = format!("f{}", self.slots.len() + 1);
                    self.slots.push(Slot {
                        handle,
                        context: context.clone(),
                        url: url.clone(),
                        name: String::new(),
                        live: true,
                    });
                }
            }
        }
    }

    /// Remember a frame's `window.name`, which the tree does not carry.
    pub fn set_name(&mut self, context: &str, name: &str) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.context == context) {
            slot.name = name.to_string();
        }
    }

    /// The slot a context has, if this session has named it.
    #[must_use]
    pub fn slot_for(&self, context: &str) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.context == context)
    }

    /// The handle for a context, if it has one.
    #[must_use]
    pub fn handle_for(&self, context: &str) -> Option<&str> {
        self.slots
            .iter()
            .find(|slot| slot.context == context)
            .map(|slot| slot.handle.as_str())
    }

    /// The slot a handle names, live or not.
    #[must_use]
    pub fn slot(&self, handle: &str) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.handle == handle)
    }

    /// Every frame named so far.
    #[must_use]
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }
}

/// The handle a spec names, if it is a handle at all. `f3`, `handle:f3` and
/// `frame:f3` are the same frame; anything else is a spec for the engine to
/// resolve as an id, an index, a URL or a name.
#[must_use]
pub fn parse_handle(spec: &str) -> Option<&str> {
    let spec = spec.trim();
    let bare = spec
        .strip_prefix("handle:")
        .or_else(|| spec.strip_prefix("frame:"))
        .unwrap_or(spec)
        .trim();
    let digits = bare.strip_prefix('f')?;
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(bare)
    } else {
        None
    }
}

/// A handle that names a frame this session no longer has.
pub(crate) fn gone(verb: &str, slot: &Slot) -> Error {
    Error::BadArgs {
        verb: verb.to_string(),
        detail: format!(
            "frame {} is gone; it was {}. Run frames for the frames this session has now",
            slot.handle,
            if slot.url.is_empty() { "never readable" } else { &slot.url }
        ),
    }
}

/// A handle this session never minted.
pub(crate) fn unknown(verb: &str, handle: &str, have: &[Slot]) -> Error {
    let known: Vec<&str> = have
        .iter()
        .filter(|slot| slot.live)
        .map(|slot| slot.handle.as_str())
        .collect();
    Error::BadArgs {
        verb: verb.to_string(),
        detail: format!(
            "no frame is named {handle}. This session has {known:?}; run frames to see them with their urls"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(context: &str, url: &str) -> (String, String) {
        (context.to_string(), url.to_string())
    }

    #[test]
    fn a_handle_survives_a_navigation_of_the_frame_it_names() {
        let mut handles = Handles::default();
        handles.refresh(&[frame("1", "https://x.test/"), frame("7", "https://y.test/a")]);
        let child = handles.handle_for("7").unwrap().to_string();
        // The frame navigates: same context, different url.
        handles.refresh(&[frame("1", "https://x.test/"), frame("7", "https://y.test/b")]);
        assert_eq!(handles.handle_for("7").unwrap(), child);
        assert_eq!(handles.slot(&child).unwrap().url, "https://y.test/b");
        assert!(handles.slot(&child).unwrap().live);
    }

    #[test]
    fn a_handle_is_never_reused_by_another_frame() {
        let mut handles = Handles::default();
        handles.refresh(&[frame("1", "https://x.test/"), frame("7", "https://y.test/")]);
        // The frame detaches and a different one attaches in its place.
        handles.refresh(&[frame("1", "https://x.test/"), frame("9", "https://z.test/")]);
        assert_eq!(handles.slot("f2").unwrap().context, "7");
        assert!(!handles.slot("f2").unwrap().live);
        assert_eq!(handles.handle_for("9").unwrap(), "f3");
        assert_eq!(handles.slots().len(), 3);
    }

    #[test]
    fn a_frame_that_comes_back_keeps_the_handle_it_had() {
        let mut handles = Handles::default();
        handles.refresh(&[frame("1", "https://x.test/"), frame("7", "https://y.test/")]);
        handles.refresh(&[frame("1", "https://x.test/")]);
        assert!(!handles.slot("f2").unwrap().live);
        handles.refresh(&[frame("1", "https://x.test/"), frame("7", "https://y.test/")]);
        assert!(handles.slot("f2").unwrap().live);
        assert_eq!(handles.slots().len(), 2);
    }

    #[test]
    fn only_a_handle_reads_as_a_handle() {
        for spec in ["f1", " f12 ", "handle:f3", "frame:f9"] {
            assert!(parse_handle(spec).is_some(), "{spec:?} is a handle");
        }
        // Everything the engine resolves itself must pass through untouched,
        // including a numeric context id and a url that starts with an f.
        for spec in ["1", "main", "index:2", "url:frame.html", "name:f", "f", "ff2", "f2x", ""] {
            assert!(parse_handle(spec).is_none(), "{spec:?} is not a handle");
        }
    }

    #[test]
    fn a_refusal_names_what_to_run() {
        let mut handles = Handles::default();
        handles.refresh(&[frame("7", "https://y.test/a")]);
        handles.refresh(&[]);
        let stale = gone("eval", handles.slot("f1").unwrap()).to_string();
        assert!(stale.contains("https://y.test/a"), "{stale}");
        assert!(stale.contains("frames"), "{stale}");
        let missing = unknown("eval", "f4", handles.slots()).to_string();
        assert!(missing.contains("f4"), "{missing}");
        assert!(missing.contains("frames"), "{missing}");
    }
}
