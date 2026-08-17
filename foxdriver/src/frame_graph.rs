//! Frame + shadow-root graph for the current page.
//!
//! Every captcha-solving operation that has to walk frames today
//! does this:
//!
//! ```text
//! for fid in page.frames().await? {
//!     if let Some(ctx) = page.frame_execution_context(fid).await? {
//!         page.evaluate_expression(...with ctx...).await?;
//!     }
//! }
//! ```
//!
//! Three problems with the bare-loop pattern:
//!
//! 1. **No structure.** `page.frames()` returns a flat list, you
//!    can't ask "which frame is this iframe's parent?", "which
//!    frames live inside the captcha container?", "which frame is
//!    nested deepest?" without re-running an extraction pass each
//!    time. Solvers re-derive the topology over and over.
//! 2. **Shadow roots are invisible.** `page.frames()` only sees
//!    cross-document boundaries; same-document shadow roots are
//!    missed. The existing in-DOM `walkAllRoots` JS pass handles
//!    them but lives in every solver as a copy-paste blob.
//! 3. **No reasoning.** With a graph you can BFS from "the deepest
//!    frame containing a captcha widget" outward to find the
//!    nearest token field, or topo-sort frames so the deepest
//!    challenge runs first. With a flat list you can't.
//!
//! [`FrameGraph`] is the substrate: snapshot once, query many
//! times. [`FrameNode`] is the per-node shape (frame_id +
//! parent + URL + title + presence of captcha markers).
//!
//! Pure data type, no IO ourselves; [`FrameGraph::snapshot`]
//! recovers the real topology with one WebDriver BiDi
//! `browsingContext.getTree` call (via [`crate::browser::Page::frame_tree`])
//! plus a per-frame eval for title/marker, then returns a built
//! graph. Tests can construct synthetic graphs without a browser.

use crate::browser::Page;
use anyhow::Result;
use std::collections::{HashMap, VecDeque};

/// One node in the frame graph.
///
/// `frame_id` is the CDP frame identifier, opaque string we hand
/// back to `page.frame_execution_context(frame_id)` when running
/// JS in this frame's context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameNode {
    /// CDP frame ID. `None` for the synthetic root that ties the
    /// main frame plus all shadow roots together; nodes representing
    /// real frames always carry one.
    pub frame_id: Option<String>,
    /// Index of the parent node in [`FrameGraph::nodes`]. `None`
    /// only for the root.
    pub parent: Option<usize>,
    /// URL of the document this frame is rendering. `about:blank`
    /// or `about:srcdoc` for synthetic / written iframes.
    pub url: String,
    /// `document.title` at snapshot time.
    pub title: String,
    /// True when the frame's body or any descendant matched a
    /// captcha-shaped selector at snapshot time. Drives reasoning
    /// like "BFS to the nearest captcha-bearing frame".
    pub has_captcha_marker: bool,
    /// Depth from the root (root = 0, top-level frame = 1, …).
    pub depth: usize,
}

/// The full graph for a page snapshot.
///
/// Children are indexed via [`FrameGraph::children`] which scans
/// the `nodes` Vec, fine for the typical <50-node frame trees we
/// see in the wild; would warrant a parent→children index for
/// 1000+-frame pages (which don't exist in practice).
#[derive(Debug, Clone, Default)]
pub struct FrameGraph {
    pub nodes: Vec<FrameNode>,
}

impl FrameGraph {
    /// Build a graph from the current state of `page`.
    ///
    /// Structure (parent links + depth + every frame's URL) comes from a
    /// single `browsingContext.getTree` round-trip via [`Page::frame_tree`],
    /// so the real cross-origin nesting is preserved, the old
    /// `page.frames()` path returned a flat id list and forced every node to
    /// `parent: root, depth: 1`, collapsing reCAPTCHA's `bframe`-inside-`anchor`
    /// (and every other nested challenge) into siblings and defeating the
    /// graph's whole purpose.
    ///
    /// Each node is then enriched with `title` + `has_captcha_marker` by
    /// evaluating [`PROBE_JS`] inside that frame's own realm, the same
    /// selector list `oracle::take_snapshot` uses, so the two passes stay in
    /// sync. A frame whose probe eval fails (raced destruction, restricted
    /// realm) is **not dropped**: it keeps its structural place with the URL
    /// `getTree` reported, and the failure is logged at `warn`, never silently
    /// swallowed (which the old path did, making cross-origin captcha frames
    /// vanish from the graph entirely).
    pub async fn snapshot(page: &Page) -> Result<Self> {
        let tree = page.frame_tree().await?;

        // IO half: probe each frame's realm for title + captcha marker. A
        // frame whose probe fails keeps its structural place (URL from
        // getTree) and the failure is logged (never silently dropped).
        let mut enriched: Vec<EnrichedFrame> = Vec::with_capacity(tree.len());
        for entry in &tree {
            let (title, has_captcha_marker) =
                match page.evaluate_in_context(PROBE_JS, &entry.id).await {
                    Ok(eval) => match eval.into_value::<FrameProbe>() {
                        Ok(v) => (v.title, v.has_captcha_marker),
                        Err(e) => {
                            tracing::warn!("frame {} probe decode failed: {e}", entry.url);
                            (String::new(), false)
                        }
                    },
                    Err(e) => {
                        tracing::warn!("frame {} probe eval failed: {e}", entry.url);
                        (String::new(), false)
                    }
                };
            enriched.push(EnrichedFrame {
                // Raw context id (not its Debug form) so the graph's `frame_id`
                // is directly usable as a `frame` target.
                id: entry.id.inner().to_string(),
                url: entry.url.clone(),
                parent: entry.parent.as_ref().map(|p| p.inner().to_string()),
                title,
                has_captcha_marker,
            });
        }

        // Pure half: reconstruct parent links + depth. Split out so it is
        // unit-testable without a browser.
        Ok(Self::assemble(&enriched))
    }

    /// Assemble the node Vec from a **pre-order** (parent-before-child) list
    /// of probed frames, recovering each node's parent index and root-relative
    /// depth from the BiDi parentage. Pure, no IO, so the linkage logic is
    /// provable on synthetic nested trees without launching a browser.
    fn assemble(entries: &[EnrichedFrame]) -> Self {
        let mut nodes: Vec<FrameNode> = Vec::with_capacity(entries.len() + 1);

        // Node 0 = synthetic root. Always present even when the page has no
        // frames; ties all top-level contexts together under one entry point.
        nodes.push(FrameNode {
            frame_id: None,
            parent: None,
            url: "(root)".into(),
            title: String::new(),
            has_captcha_marker: false,
            depth: 0,
        });

        // Context id → node index, so a child resolves its parent's index.
        // Pre-order guarantees the parent is inserted before any of its
        // children are processed.
        let mut id_to_idx: HashMap<String, usize> = HashMap::new();

        for e in entries {
            let parent_idx = match &e.parent {
                None => 0, // top-level context hangs off the synthetic root
                Some(pid) => match id_to_idx.get(pid) {
                    Some(&idx) => idx,
                    None => {
                        // Pre-order should make this impossible; if a tree ever
                        // arrives out of order, say so loudly rather than
                        // silently reparenting to root.
                        tracing::warn!(
                            "frame parent {pid} not seen before child {}, attaching to root",
                            e.id
                        );
                        0
                    }
                },
            };
            let depth = nodes[parent_idx].depth + 1;
            id_to_idx.insert(e.id.clone(), nodes.len());
            nodes.push(FrameNode {
                frame_id: Some(e.id.clone()),
                parent: Some(parent_idx),
                url: e.url.clone(),
                title: e.title.clone(),
                has_captcha_marker: e.has_captcha_marker,
                depth,
            });
        }

        Self { nodes }
    }

    /// True iff the graph has any node with `has_captcha_marker`.
    /// Cheap pre-check before more expensive walks.
    pub fn any_captcha_marker(&self) -> bool {
        self.nodes.iter().any(|n| n.has_captcha_marker)
    }

    /// Indices of all child nodes of `parent_idx`.
    ///
    /// Linear scan; acceptable for typical tree sizes (<50 nodes).
    /// If we ever ship a graph with hundreds of nodes, replace
    /// with a precomputed parent→children index.
    pub fn children(&self, parent_idx: usize) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                if n.parent == Some(parent_idx) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    /// BFS from the root, returning node indices in visit order.
    ///
    /// Handy for "do this thing in every frame, top-down" without
    /// open-coding the queue management at every call site.
    pub fn bfs(&self) -> Vec<usize> {
        if self.nodes.is_empty() {
            return Vec::new();
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(0);
        while let Some(idx) = queue.pop_front() {
            order.push(idx);
            for child in self.children(idx) {
                queue.push_back(child);
            }
        }
        order
    }

    /// Find the deepest node that has a captcha marker. Returns
    /// `None` when no node carries one.
    ///
    /// Useful for "walk OUTWARD from the captcha to find the
    /// nearest enclosing token field", once you have the captcha
    /// node, traverse parent links upward until the token shows up.
    pub fn deepest_captcha(&self) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.has_captcha_marker)
            .max_by_key(|(_, n)| n.depth)
            .map(|(i, _)| i)
    }

    /// All node indices on the path from `node_idx` up to the root,
    /// inclusive of both endpoints. Empty when `node_idx` is OOB.
    ///
    /// Use this when a captcha solve produces a token in a deeply-
    /// nested iframe and you need to relay it up the tree via
    /// `postMessage`: the path is the relay route.
    pub fn ancestors_inclusive(&self, mut node_idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut visited = std::collections::HashSet::new();
        while let Some(node) = self.nodes.get(node_idx) {
            if !visited.insert(node_idx) {
                // Cycle guard. Snapshot graphs are trees by
                // construction but defensive coding wins.
                break;
            }
            out.push(node_idx);
            match node.parent {
                Some(p) => node_idx = p,
                None => break,
            }
        }
        out
    }

    /// Group nodes by URL host, returning a map host → indices.
    ///
    /// Lets a solver target "all cross-origin frames hosted by
    /// `challenges.cloudflare.com`" in one query, e.g. to pierce
    /// the CF Turnstile sandbox.
    pub fn frames_by_host(&self) -> HashMap<String, Vec<usize>> {
        let mut out: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(host) = url::Url::parse(&n.url)
                .ok()
                .and_then(|u| u.host_str().map(String::from))
            {
                out.entry(host).or_default().push(i);
            }
        }
        out
    }
}

#[derive(serde::Deserialize)]
struct FrameProbe {
    title: String,
    has_captcha_marker: bool,
}

/// A frame with its structure (id/url/parent-id) plus probed
/// title/marker, before [`FrameGraph::assemble`] turns parent ids into
/// node indices. `parent` is the parent frame's context id, `None` for a
/// top-level context.
struct EnrichedFrame {
    id: String,
    url: String,
    parent: Option<String>,
    title: String,
    has_captcha_marker: bool,
}

/// JS payload run inside every frame's execution context to
/// populate a [`FrameNode`]'s `title` + `has_captcha_marker` (the
/// URL comes from `browsingContext.getTree`, authoritative even for
/// cross-origin frames). Selector list mirrors `oracle::take_snapshot`
/// so the two passes stay consistent.
const PROBE_JS: &str = r#"(function() {
    try {
        const title = (document && document.title) ? document.title : '';
        const el = (document && document.querySelector) ? document.querySelector(
            'iframe[src*="challenges.cloudflare.com"], iframe[src*="recaptcha"], iframe[src*="hcaptcha"], '
            + 'iframe[src*="arkoselabs"], iframe[src*="datadome"], iframe[src*="geetest"], '
            + 'iframe[src*="perimeterx"], iframe[src*="kasada"], iframe[src*="incapsula"], '
            + '.cf-turnstile, .h-captcha, .g-recaptcha, '
            + '#challenge-form, #challenge-stage, #cf-please-wait, #px-captcha, '
            + '[id^="captcha"], [class*="captcha" i], [class*="challenge" i]'
        ) : null;
        return { title: title, has_captcha_marker: !!el };
    } catch(e) {
        return { title: '', has_captcha_marker: false };
    }
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic graph for testing without a browser:
    ///
    /// ```text
    /// root
    /// ├── main (no captcha)
    /// │   ├── frame_a (captcha)
    /// │   │   └── frame_aa (captcha, deepest)
    /// │   └── frame_b
    /// └── isolated (no parent linkage)
    /// ```
    fn fixture_graph() -> FrameGraph {
        FrameGraph {
            nodes: vec![
                // 0: root
                FrameNode {
                    frame_id: None,
                    parent: None,
                    url: "(root)".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 0,
                },
                // 1: main
                FrameNode {
                    frame_id: Some("F1".into()),
                    parent: Some(0),
                    url: "https://example.com".into(),
                    title: "Main".into(),
                    has_captcha_marker: false,
                    depth: 1,
                },
                // 2: frame_a (captcha)
                FrameNode {
                    frame_id: Some("F2".into()),
                    parent: Some(1),
                    url: "https://challenges.cloudflare.com/turnstile".into(),
                    title: String::new(),
                    has_captcha_marker: true,
                    depth: 2,
                },
                // 3: frame_aa (captcha, deepest)
                FrameNode {
                    frame_id: Some("F3".into()),
                    parent: Some(2),
                    url: "https://challenges.cloudflare.com/turnstile/inner".into(),
                    title: String::new(),
                    has_captcha_marker: true,
                    depth: 3,
                },
                // 4: frame_b
                FrameNode {
                    frame_id: Some("F4".into()),
                    parent: Some(1),
                    url: "https://example.com/sidebar".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 2,
                },
            ],
        }
    }

    #[test]
    fn empty_graph_has_no_captcha_markers() {
        let g = FrameGraph::default();
        assert!(!g.any_captcha_marker());
        assert!(g.bfs().is_empty());
        assert!(g.deepest_captcha().is_none());
    }

    #[test]
    fn any_captcha_marker_short_circuits_on_first_match() {
        let g = fixture_graph();
        assert!(g.any_captcha_marker());
    }

    #[test]
    fn children_returns_all_direct_children_of_root() {
        let g = fixture_graph();
        let kids = g.children(0);
        assert_eq!(kids, vec![1]);
    }

    #[test]
    fn children_returns_all_direct_children_of_internal_node() {
        let g = fixture_graph();
        // Main (idx=1) has frame_a (2) and frame_b (4).
        let kids = g.children(1);
        assert_eq!(kids, vec![2, 4]);
    }

    #[test]
    fn bfs_visits_root_first_then_each_level() {
        let g = fixture_graph();
        let order = g.bfs();
        // Expected: 0 (root) → 1 (main) → 2,4 (children of main)
        // → 3 (child of frame_a). Order within a level matches
        // insertion order in `nodes`.
        assert_eq!(order, vec![0, 1, 2, 4, 3]);
    }

    #[test]
    fn deepest_captcha_finds_innermost_marker() {
        let g = fixture_graph();
        let deepest = g.deepest_captcha().expect("fixture has captcha markers");
        assert_eq!(deepest, 3, "frame_aa is the deepest captcha-bearing node");
    }

    #[test]
    fn ancestors_inclusive_walks_to_root_in_order() {
        let g = fixture_graph();
        // From frame_aa (3) → frame_a (2) → main (1) → root (0).
        let path = g.ancestors_inclusive(3);
        assert_eq!(path, vec![3, 2, 1, 0]);
    }

    #[test]
    fn ancestors_inclusive_handles_oob_index_gracefully() {
        let g = fixture_graph();
        assert!(g.ancestors_inclusive(999).is_empty());
    }

    #[test]
    fn ancestors_inclusive_handles_root_node() {
        let g = fixture_graph();
        let path = g.ancestors_inclusive(0);
        assert_eq!(path, vec![0]);
    }

    #[test]
    fn frames_by_host_groups_correctly() {
        let g = fixture_graph();
        let hosts = g.frames_by_host();
        assert_eq!(
            hosts.get("example.com").map(|v| v.len()),
            Some(2),
            "main + sidebar both on example.com"
        );
        assert_eq!(
            hosts.get("challenges.cloudflare.com").map(|v| v.len()),
            Some(2),
            "two CF turnstile frames"
        );
    }

    #[test]
    fn frames_by_host_skips_unparseable_urls() {
        // (root) URL is not a valid http(s) URL → must be skipped.
        let g = fixture_graph();
        let hosts = g.frames_by_host();
        assert!(!hosts.contains_key("(root)"));
    }

    #[test]
    fn children_leaf_node_returns_empty() {
        let g = fixture_graph();
        // frame_aa (3) is a leaf (no children).
        assert!(g.children(3).is_empty());
    }

    #[test]
    fn children_oob_returns_empty() {
        let g = fixture_graph();
        assert!(g.children(999).is_empty());
    }

    #[test]
    fn bfs_single_node() {
        let g = FrameGraph {
            nodes: vec![FrameNode {
                frame_id: None,
                parent: None,
                url: "solo".into(),
                title: String::new(),
                has_captcha_marker: false,
                depth: 0,
            }],
        };
        assert_eq!(g.bfs(), vec![0]);
    }

    #[test]
    fn bfs_linear_chain() {
        let g = FrameGraph {
            nodes: vec![
                FrameNode {
                    frame_id: Some("A".into()),
                    parent: None,
                    url: "a".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 0,
                },
                FrameNode {
                    frame_id: Some("B".into()),
                    parent: Some(0),
                    url: "b".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 1,
                },
                FrameNode {
                    frame_id: Some("C".into()),
                    parent: Some(1),
                    url: "c".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 2,
                },
            ],
        };
        assert_eq!(g.bfs(), vec![0, 1, 2]);
    }

    #[test]
    fn deepest_captcha_none_when_no_markers() {
        let g = FrameGraph {
            nodes: vec![
                FrameNode {
                    frame_id: None,
                    parent: None,
                    url: "root".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 0,
                },
                FrameNode {
                    frame_id: Some("A".into()),
                    parent: Some(0),
                    url: "a".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 1,
                },
            ],
        };
        assert!(g.deepest_captcha().is_none());
    }

    #[test]
    fn deepest_captcha_prefers_last_at_same_depth() {
        let g = FrameGraph {
            nodes: vec![
                FrameNode {
                    frame_id: None,
                    parent: None,
                    url: "root".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 0,
                },
                FrameNode {
                    frame_id: Some("A".into()),
                    parent: Some(0),
                    url: "a".into(),
                    title: String::new(),
                    has_captcha_marker: true,
                    depth: 1,
                },
                FrameNode {
                    frame_id: Some("B".into()),
                    parent: Some(0),
                    url: "b".into(),
                    title: String::new(),
                    has_captcha_marker: true,
                    depth: 1,
                },
            ],
        };
        // Both at depth 1; iteration order means B (index 2) wins.
        assert_eq!(g.deepest_captcha(), Some(2));
    }

    #[test]
    fn ancestors_inclusive_orphaned_node_stops_at_root() {
        // A node whose parent index doesn't exist should still be included
        // and then stop because the parent lookup fails.
        let g = FrameGraph {
            nodes: vec![
                FrameNode {
                    frame_id: None,
                    parent: None,
                    url: "root".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 0,
                },
                FrameNode {
                    frame_id: Some("orphan".into()),
                    parent: Some(999),
                    url: "orphan".into(),
                    title: String::new(),
                    has_captcha_marker: false,
                    depth: 1,
                },
            ],
        };
        let path = g.ancestors_inclusive(1);
        assert_eq!(path, vec![1]);
    }

    #[test]
    fn frames_by_host_empty_graph() {
        let g = FrameGraph::default();
        assert!(g.frames_by_host().is_empty());
    }

    #[test]
    fn frames_by_host_with_port() {
        let g = FrameGraph {
            nodes: vec![FrameNode {
                frame_id: None,
                parent: None,
                url: "http://localhost:8080/path".into(),
                title: String::new(),
                has_captcha_marker: false,
                depth: 0,
            }],
        };
        let hosts = g.frames_by_host();
        assert_eq!(hosts.get("localhost").map(|v| v.len()), Some(1));
    }

    #[test]
    fn frames_by_host_ip_address() {
        let g = FrameGraph {
            nodes: vec![FrameNode {
                frame_id: None,
                parent: None,
                url: "http://192.168.1.1/admin".into(),
                title: String::new(),
                has_captcha_marker: false,
                depth: 0,
            }],
        };
        let hosts = g.frames_by_host();
        assert_eq!(hosts.get("192.168.1.1").map(|v| v.len()), Some(1));
    }

    // ── assemble(): the parent-linkage + depth reconstruction that the old
    //    flat-snapshot path never produced ──────────────────────────────────

    fn enriched(id: &str, parent: Option<&str>, url: &str, captcha: bool) -> EnrichedFrame {
        EnrichedFrame {
            id: id.into(),
            url: url.into(),
            parent: parent.map(Into::into),
            title: String::new(),
            has_captcha_marker: captcha,
        }
    }

    /// The reCAPTCHA topology that defeats a flat snapshot: the cross-origin
    /// `bframe` (the challenge) is nested INSIDE the `anchor` (the checkbox),
    /// which is itself nested inside the main document. A flat snapshot would
    /// make all three siblings at depth 1; `assemble` must recover the chain.
    #[test]
    fn assemble_recovers_nested_recaptcha_depth_not_a_flat_tree() {
        // Pre-order BiDi walk: main → anchor → bframe.
        let entries = vec![
            enriched("MAIN", None, "https://victim.example/login", false),
            enriched(
                "ANCHOR",
                Some("MAIN"),
                "https://www.google.com/recaptcha/api2/anchor",
                false,
            ),
            enriched(
                "BFRAME",
                Some("ANCHOR"),
                "https://www.google.com/recaptcha/api2/bframe",
                true,
            ),
        ];
        let g = FrameGraph::assemble(&entries);

        // root + 3 frames.
        assert_eq!(g.nodes.len(), 4);

        // Depths are NOT all 1 (the old-bug signature) (they form a chain).
        assert_eq!(g.nodes[0].depth, 0, "synthetic root");
        assert_eq!(g.nodes[1].depth, 1, "main document under root");
        assert_eq!(g.nodes[2].depth, 2, "anchor nested in main");
        assert_eq!(g.nodes[3].depth, 3, "bframe nested in anchor");

        // Parent indices point up the real chain, not all at root.
        assert_eq!(g.nodes[1].parent, Some(0));
        assert_eq!(g.nodes[2].parent, Some(1));
        assert_eq!(g.nodes[3].parent, Some(2));

        // The challenge bframe is the deepest captcha-bearing node, and walking
        // its ancestors yields the full pierce path the solver needs.
        let deepest = g.deepest_captcha().expect("bframe carries the marker");
        assert_eq!(deepest, 3);
        assert_eq!(g.ancestors_inclusive(deepest), vec![3, 2, 1, 0]);

        // frame_id is the raw context id (directly usable as a frame target),
        // not a Debug-formatted blob.
        assert_eq!(g.nodes[3].frame_id.as_deref(), Some("BFRAME"));
    }

    /// Two sibling iframes under the main document must stay siblings (same
    /// parent + depth), distinct from the nesting case above.
    #[test]
    fn assemble_keeps_true_siblings_at_the_same_depth() {
        let entries = vec![
            enriched("MAIN", None, "https://site.example/", false),
            enriched("ADS", Some("MAIN"), "https://ads.example/slot", false),
            enriched("CHAT", Some("MAIN"), "https://chat.example/widget", false),
        ];
        let g = FrameGraph::assemble(&entries);

        assert_eq!(
            g.children(1),
            vec![2, 3],
            "both iframes are children of main"
        );
        assert_eq!(g.nodes[2].depth, 2);
        assert_eq!(g.nodes[3].depth, 2);
    }

    /// Multiple top-level contexts (e.g. several tabs) all hang off the
    /// synthetic root at depth 1.
    #[test]
    fn assemble_attaches_each_top_level_context_to_the_root() {
        let entries = vec![
            enriched("TAB1", None, "https://a.example/", false),
            enriched("TAB2", None, "https://b.example/", false),
        ];
        let g = FrameGraph::assemble(&entries);

        assert_eq!(g.children(0), vec![1, 2]);
        assert_eq!(g.nodes[1].depth, 1);
        assert_eq!(g.nodes[2].depth, 1);
    }

    /// An empty page still yields the stable synthetic root.
    #[test]
    fn assemble_empty_tree_is_just_the_root() {
        let g = FrameGraph::assemble(&[]);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].frame_id, None);
        assert_eq!(g.nodes[0].depth, 0);
    }
}
