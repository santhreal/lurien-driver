//! The vendor catalog, compiled from `captcha/kinds/*.toml` by `build.rs`.
//!
//! No vendor name appears in this file. A new vendor is a TOML file; the probe
//! selectors, the token hooks, and the kind mapping all follow from it, so the
//! CLI, the MCP server, the HTTP face, and the engine cannot disagree about what
//! a page is.

/// One vendor binding: how to recognize the widget, and where its token lands.
#[derive(Debug, Clone, Copy)]
pub struct VendorBinding {
    /// Vendor identifier from the TOML.
    pub name: &'static str,
    /// Closed kind this vendor presents.
    pub kind: &'static str,
    /// File the binding came from.
    pub source: &'static str,
    /// Where the widget's actionable element lives, in chrome-visible language.
    pub target: &'static str,
    /// Substrings that identify the widget's iframe.
    pub iframe_src: &'static [&'static str],
    /// Custom element names the widget defines.
    pub custom_elements: &'static [&'static str],
    /// Extra CSS selectors that identify the widget.
    pub selectors: &'static [&'static str],
    /// Cookies whose presence proves the challenge was cleared.
    pub cookies: &'static [&'static str],
    /// Script URL substrings the vendor loads.
    pub scripts: &'static [&'static str],
    /// Form fields the solved token is written into.
    pub token_inputs: &'static [&'static str],
    /// Cookies the solved token is written into.
    pub token_cookies: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

/// Kinds that resolve to a bounded token wait rather than an interactive solve.
/// A widget of one of these kinds means "wait", not "act".
const SCORE_LIKE: &[&str] = &["score", "checkbox"];

/// Selector matching any field a solved token is written into.
#[must_use]
pub fn token_selector() -> String {
    let mut parts = Vec::new();
    for binding in CATALOG {
        for field in binding.token_inputs {
            parts.push(format!("input[name=\"{field}\"]"));
            parts.push(format!("textarea[name=\"{field}\"]"));
        }
    }
    parts.sort();
    parts.dedup();
    parts.join(", ")
}

/// Every cookie whose presence means a challenge was already cleared.
#[must_use]
pub fn cleared_cookies() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = CATALOG
        .iter()
        .flat_map(|b| b.token_cookies.iter().chain(b.cookies.iter()).copied())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Selector matching the widget of any vendor presenting `kind`.
#[must_use]
pub fn widget_selector(kind: &str) -> String {
    let mut parts = Vec::new();
    for binding in CATALOG.iter().filter(|b| b.kind == kind) {
        for src in binding.iframe_src {
            parts.push(format!("iframe[src*=\"{src}\"]"));
        }
        for element in binding.custom_elements {
            parts.push((*element).to_string());
        }
        for selector in binding.selectors {
            parts.push((*selector).to_string());
        }
    }
    parts.sort();
    parts.dedup();
    parts.join(", ")
}

/// Kinds with at least one widget selector, in probe order: score-like first, so
/// a managed challenge is a token wait and never a false interactive claim.
#[must_use]
pub fn probe_kinds() -> Vec<&'static str> {
    let mut interactive: Vec<&'static str> = CATALOG
        .iter()
        .map(|b| b.kind)
        .filter(|k| !SCORE_LIKE.contains(k))
        .collect();
    interactive.sort_unstable();
    interactive.dedup();
    let mut kinds: Vec<&'static str> = SCORE_LIKE
        .iter()
        .copied()
        .filter(|k| CATALOG.iter().any(|b| b.kind == *k))
        .collect();
    kinds.extend(interactive);
    kinds
}

/// Whether a widget of `kind` means "wait for a token" rather than "solve".
#[must_use]
pub fn is_score_like(kind: &str) -> bool {
    SCORE_LIKE.contains(&kind)
}

/// The catalog as the engine receives it.
///
/// The engine holds no TOML reader: this crate parses `captcha/kinds/` once at
/// build time and hands the result over, so a vendor row cannot mean one thing
/// to the driver and another to the browser that paints the widget.
#[must_use]
pub fn catalog_json() -> serde_json::Value {
    serde_json::Value::Array(
        CATALOG
            .iter()
            .map(|b| {
                serde_json::json!({
                    "name": b.name,
                    "kind": b.kind,
                    "source": b.source,
                    "target": b.target,
                    "iframe_src": b.iframe_src,
                    "custom_elements": b.custom_elements,
                    "selectors": b.selectors,
                    "cookies": b.cookies,
                    "scripts": b.scripts,
                    "token_inputs": b.token_inputs,
                    "token_cookies": b.token_cookies,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_not_empty_and_every_entry_is_complete() {
        assert!(CATALOG.len() >= 3, "catalog has {} entries", CATALOG.len());
        for binding in CATALOG {
            assert!(!binding.name.is_empty(), "{} has no name", binding.source);
            assert!(!binding.kind.is_empty(), "{} has no kind", binding.source);
            let recognizable = !binding.iframe_src.is_empty()
                || !binding.custom_elements.is_empty()
                || !binding.selectors.is_empty()
                || !binding.cookies.is_empty();
            assert!(
                recognizable,
                "{} cannot be recognized on a page",
                binding.source
            );
            assert!(
                !binding.token_inputs.is_empty() || !binding.token_cookies.is_empty(),
                "{} has no token hook, so a solve could never be proven",
                binding.source
            );
        }
    }

    #[test]
    fn a_token_selector_is_built_for_every_token_field() {
        let selector = token_selector();
        for binding in CATALOG {
            for field in binding.token_inputs {
                assert!(
                    selector.contains(field),
                    "{} token field {field} is not in the probe",
                    binding.source
                );
            }
        }
    }

    #[test]
    fn score_like_kinds_are_probed_before_interactive_ones() {
        // A managed challenge that also paints a widget must resolve to a token
        // wait. Probing an interactive kind first would claim a solve the
        // product does not do.
        let kinds = probe_kinds();
        assert!(!kinds.is_empty());
        let first_interactive = kinds.iter().position(|k| !is_score_like(k));
        let last_score = kinds.iter().rposition(|k| is_score_like(k));
        if let (Some(first), Some(last)) = (first_interactive, last_score) {
            assert!(last < first, "probe order is wrong: {kinds:?}");
        }
    }

    #[test]
    fn every_probed_kind_has_a_selector() {
        for kind in probe_kinds() {
            assert!(
                !widget_selector(kind).is_empty(),
                "kind {kind} is probed with an empty selector"
            );
        }
    }

    #[test]
    fn no_vendor_name_is_needed_to_read_the_catalog() {
        // The catalog is addressed by kind, never by vendor. This test exists to
        // fail if someone adds a by-name lookup helper here.
        let source = include_str!("catalog.rs");
        for binding in CATALOG {
            assert!(
                !source.contains(binding.name),
                "catalog.rs names vendor {}; vendors live in captcha/kinds/",
                binding.name
            );
        }
    }
}
