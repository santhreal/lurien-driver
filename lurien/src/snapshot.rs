//! The page as a list of addressable nodes, and the handles that address them.
//!
//! An agent pays for a page by the token. Page source is the worst possible
//! representation of it: mostly markup it cannot act on, and different after
//! every redesign even when the page still does the same thing. So the default
//! snapshot is a role/name list, in document order, with one handle per node:
//!
//! ```text
//! - heading "Sign in" [level=1] [ref=e1]
//! - textbox "Email" [ref=e2]
//! - button "Log in" [ref=e3]
//! ```
//!
//! A handle goes back to any verb that takes a selector, as `ref:e3`. The
//! handle table lives in the driver, so nothing in the page is tagged, and a
//! handle is checked against the role and name it was captured with before it is
//! acted on: a page that re-rendered underneath a handle earns a refusal, not a
//! click on whatever moved into place.

use crate::error::Error;
use runtime_foxdriver::Page;

/// The walker, evaluated in the page after the resolver it borrows from.
const WALKER: &str = include_str!("snapshot.js");

/// How many nodes a snapshot reports before it says it stopped.
pub const DEFAULT_LIMIT: usize = 200;

/// One addressable node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The handle a caller sends back, without its `ref:` prefix.
    pub handle: String,
    /// Role, explicit or implicit.
    pub role: String,
    /// Accessible name, or the node's own text when it has no name.
    pub name: String,
    /// CSS path, used to act and to check the handle still means this node.
    pub path: String,
    /// Nesting depth among reported nodes.
    pub depth: usize,
    /// Flags worth a token: `disabled`, `checked`, `level=2`, `required`.
    pub state: Vec<String>,
    /// Current value of a control, when it has one and is not a password.
    pub value: Option<String>,
}

/// One capture of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Document title.
    pub title: String,
    /// Document URL.
    pub url: String,
    /// Nodes in document order.
    pub nodes: Vec<Node>,
    /// Nodes past the limit, reported rather than hidden.
    pub dropped: usize,
}

/// Walk the page and assign a handle to every addressable node.
pub async fn capture(page: &Page, limit: usize) -> Result<Snapshot, Error> {
    let script = format!(
        "(() => {{ {resolver}\n{walker}\n return lurienSnapshot({limit}); }})()",
        resolver = crate::locator::RESOLVER,
        walker = WALKER,
    );
    let answer: String = page
        .evaluate(&script)
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .into_value()
        .map_err(|e| Error::Other(format!("snapshot did not answer: {e}")))?;
    parse(&answer)
}

/// Does `node`'s handle still point at the node it was captured for?
///
/// The failure is deliberately not "no such element": a stale handle and a
/// missing element need different corrections, and only one of them is fixed by
/// taking a new snapshot.
pub async fn verify(page: &Page, node: &Node) -> Result<(), Error> {
    let script = format!(
        "(() => {{ {resolver}\n{walker}\n return lurienVerify({path}, {role}, {name}); }})()",
        resolver = crate::locator::RESOLVER,
        walker = WALKER,
        path = json_string(&node.path),
        role = json_string(&node.role),
        name = json_string(&node.name),
    );
    let answer: String = page
        .evaluate(&script)
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .into_value()
        .map_err(|e| Error::Other(format!("handle check did not answer: {e}")))?;
    let answer: serde_json::Value =
        serde_json::from_str(&answer).map_err(|e| Error::Other(e.to_string()))?;
    if answer["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    let detail = match answer["why"].as_str().unwrap_or("gone") {
        "changed" => format!(
            "the page changed under the handle: {} {:?} is now {} {:?}",
            node.role,
            node.name,
            answer["role"].as_str().unwrap_or("?"),
            answer["name"].as_str().unwrap_or("")
        ),
        "invalid" => format!(
            "the handle's path no longer parses ({})",
            answer["detail"].as_str().unwrap_or("rejected by the page")
        ),
        _ => format!("the {} {:?} it named is gone", node.role, node.name),
    };
    Err(Error::Unresolved {
        selector: format!("ref:{}", node.handle),
        detail,
        waited_ms: 0,
        action: "take a fresh snapshot and use the handle it reports".to_string(),
    })
}

impl Snapshot {
    /// The representation an agent reads: one line per node, indented by depth.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!("title: {}\nurl: {}", self.title, self.url);
        for node in &self.nodes {
            out.push('\n');
            for _ in 0..node.depth {
                out.push_str("  ");
            }
            out.push_str("- ");
            out.push_str(&node.role);
            if !node.name.is_empty() {
                out.push_str(&format!(" {:?}", node.name));
            }
            if let Some(value) = &node.value {
                out.push_str(&format!(" [value={value:?}]"));
            }
            for flag in &node.state {
                out.push_str(&format!(" [{flag}]"));
            }
            out.push_str(&format!(" [ref={}]", node.handle));
        }
        if self.dropped > 0 {
            out.push_str(&format!(
                "\n\n{} more node(s) past the limit. Raise `limit` to see them.",
                self.dropped
            ));
        }
        out.push('\n');
        out
    }

    /// The node a handle names, with or without its `ref:` prefix.
    #[must_use]
    pub fn node(&self, handle: &str) -> Option<&Node> {
        let handle = handle.strip_prefix("ref:").unwrap_or(handle);
        self.nodes.iter().find(|n| n.handle == handle)
    }

    /// Handles a caller can use, for the error when one is unknown.
    #[must_use]
    pub fn handles(&self) -> String {
        match (self.nodes.first(), self.nodes.last()) {
            (Some(first), Some(last)) if first.handle != last.handle => {
                format!("{}..{}", first.handle, last.handle)
            }
            (Some(only), _) => only.handle.clone(),
            _ => "none: the snapshot was empty".to_string(),
        }
    }
}

fn parse(raw: &str) -> Result<Snapshot, Error> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| Error::Other(format!("snapshot is not JSON: {e}")))?;
    let nodes = value["nodes"]
        .as_array()
        .map(|rows| rows.iter().map(node_from).collect())
        .unwrap_or_default();
    Ok(Snapshot {
        title: value["title"].as_str().unwrap_or_default().to_string(),
        url: value["url"].as_str().unwrap_or_default().to_string(),
        nodes,
        dropped: value["dropped"].as_u64().unwrap_or(0) as usize,
    })
}

fn node_from(row: &serde_json::Value) -> Node {
    Node {
        handle: row["ref"].as_str().unwrap_or_default().to_string(),
        role: row["role"].as_str().unwrap_or("generic").to_string(),
        name: row["name"].as_str().unwrap_or_default().to_string(),
        path: row["path"].as_str().unwrap_or_default().to_string(),
        depth: row["depth"].as_u64().unwrap_or(0) as usize,
        state: row["state"]
            .as_array()
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        value: row["value"].as_str().map(str::to_string),
    }
}

fn json_string(raw: &str) -> String {
    serde_json::Value::String(raw.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Snapshot {
        parse(
            r#"{"title":"Sign in","url":"https://example.com/login","dropped":3,"nodes":[
              {"ref":"e1","role":"heading","name":"Sign in","path":"h1","depth":0,"state":["level=1"]},
              {"ref":"e2","role":"textbox","name":"Email","path":"form > input","depth":1,
               "state":["required"],"value":"me@example.com"},
              {"ref":"e3","role":"button","name":"Log in","path":"form > button","depth":1,
               "state":["disabled"]}
            ]}"#,
        )
        .expect("the fixture is JSON")
    }

    /// A snapshot is the representation an agent acts from, so every line has to
    /// carry the three things an act needs: what the node is, what it is called,
    /// and how to address it.
    #[test]
    fn every_line_names_a_role_a_name_and_a_handle() {
        let rendered = page().render();
        for line in rendered.lines().filter(|l| l.trim_start().starts_with("- ")) {
            assert!(
                line.contains("[ref=e"),
                "a node with no handle cannot be acted on: {line}"
            );
        }
        assert!(rendered.contains("- heading \"Sign in\" [level=1] [ref=e1]"), "{rendered}");
        assert!(
            rendered.contains("  - textbox \"Email\" [value=\"me@example.com\"] [required] [ref=e2]"),
            "{rendered}"
        );
        assert!(rendered.contains("  - button \"Log in\" [disabled] [ref=e3]"), "{rendered}");
    }

    /// A page cut off at the limit must say so. A snapshot that silently ends is
    /// read as "the page has nothing else", which is how an agent concludes a
    /// button does not exist.
    #[test]
    fn a_truncated_snapshot_says_what_it_dropped() {
        let rendered = page().render();
        assert!(rendered.contains("3 more node(s) past the limit"), "{rendered}");
        let whole = Snapshot { dropped: 0, ..page() };
        assert!(!whole.render().contains("past the limit"));
    }

    /// The handle is the whole point of the representation: it has to come back
    /// in either form a caller might send it.
    #[test]
    fn a_handle_resolves_with_or_without_its_prefix() {
        let snap = page();
        assert_eq!(snap.node("e2").map(|n| n.role.as_str()), Some("textbox"));
        assert_eq!(snap.node("ref:e2").map(|n| n.path.as_str()), Some("form > input"));
        assert!(snap.node("e99").is_none());
        assert_eq!(snap.handles(), "e1..e3");
    }

    /// Depth is what makes the list a map of the page rather than a pile of
    /// controls: a caller reads which nodes belong to which region.
    #[test]
    fn nesting_is_indented() {
        let rendered = page().render();
        let email = rendered
            .lines()
            .find(|l| l.contains("\"Email\""))
            .expect("the field is rendered");
        assert!(email.starts_with("  - "), "a nested node is indented: {email:?}");
        let heading = rendered
            .lines()
            .find(|l| l.contains("\"Sign in\""))
            .expect("the heading is rendered");
        assert!(heading.starts_with("- "), "a top-level node is not: {heading:?}");
    }

    /// An empty page is a legitimate answer and must not read as an error or
    /// invent a handle.
    #[test]
    fn an_empty_page_renders_its_title_and_nothing_else() {
        let snap = parse(r#"{"title":"","url":"about:blank","nodes":[],"dropped":0}"#)
            .expect("the fixture is JSON");
        assert_eq!(snap.render(), "title: \nurl: about:blank\n");
        assert_eq!(snap.handles(), "none: the snapshot was empty");
    }
}
