//! Verb registry: one file per verb, one spec per verb, one dispatcher.
//!
//! A face (CLI, MCP, HTTP) is a transport. It never matches on a verb name and
//! never learns a verb's arguments: it reads both off the [`VerbSpec`]. That is
//! why `lurien` and `lurien-mcp` cannot drift, and why a new verb is a new file
//! plus one line in its domain's `SPECS`.

pub mod args;
pub mod context;
pub mod dialog;
pub mod dom;
pub mod frame;
pub mod input;
pub mod intercept;
pub mod net;
pub mod observe;
pub mod page;
pub mod profile;
pub mod schema;
pub mod session;
pub mod state;
pub mod storage;

pub use args::Args;

use crate::error::Error;
use crate::session::Session;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

/// Verb family. One directory per domain under `src/verb/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Domain {
    /// Document lifecycle: navigate, reload, history, wait, capture.
    Page,
    /// Element-scoped work: click, fill, select, upload, read.
    Dom,
    /// Raw trusted input: keys, wheel, pointer.
    Input,
    /// Browsing-context tree and cross-origin frame work.
    Frame,
    /// Cookies and web storage.
    Storage,
    /// Whole-origin state snapshots: cookies plus web storage.
    State,
    /// Captured network traffic, redacted.
    Net,
    /// JavaScript dialogs and downloads.
    Dialog,
    /// Passive page telemetry: console, errors, CSP, postMessage, DOM sinks.
    Observe,
    /// Persona and real-profile import.
    Profile,
    /// Browsing-context lifecycle: list, create, switch, close.
    Context,
    /// Sequences of verbs: work about the calls rather than about the page.
    Session,
    /// Request/response interception and header manipulation.
    Intercept,
}

impl Domain {
    /// Lowercase directory name. Also the docs section and the dotted alias prefix.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Dom => "dom",
            Self::Input => "input",
            Self::Frame => "frame",
            Self::Storage => "storage",
            Self::State => "state",
            Self::Net => "net",
            Self::Dialog => "dialog",
            Self::Observe => "observe",
            Self::Profile => "profile",
            Self::Context => "context",
            Self::Session => "session",
            Self::Intercept => "intercept",
        }
    }

    /// Every domain, for docs and coverage tests.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Page,
            Self::Dom,
            Self::Input,
            Self::Frame,
            Self::Storage,
            Self::State,
            Self::Net,
            Self::Dialog,
            Self::Observe,
            Self::Profile,
            Self::Context,
            Self::Session,
            Self::Intercept,
        ]
    }

    /// One sentence naming when a caller reaches for this family. Composed into
    /// the MCP tool description and `--help`, so an agent picking between two
    /// verbs reads the same guidance a person does.
    #[must_use]
    pub const fn guidance(self) -> &'static str {
        match self {
            Self::Page => {
                "Use this when the subject is the whole document: navigating, waiting for it, \
                 reading it, or capturing it."
            }
            Self::Dom => {
                "Use this when the subject is one element on the page, named by a selector."
            }
            Self::Input => {
                "Use this for input the page must believe came from a hand, when no element \
                 is being targeted."
            }
            Self::Frame => {
                "Use this when the element you want lives in an iframe rather than the top \
                 document."
            }
            Self::Storage => "Use this to read or write cookies and web storage directly.",
            Self::State => {
                "Use this to carry a whole logged-in origin between sessions in one blob."
            }
            Self::Net => {
                "Use this to read what the page requested, after redaction, rather than to \
                 change it."
            }
            Self::Dialog => {
                "Use this when a native dialog or a download is blocking the page and must be \
                 answered."
            }
            Self::Observe => {
                "Use this to read what the page did on its own: console, errors, CSP, messages, \
                 DOM sinks."
            }
            Self::Profile => {
                "Use this to choose or inspect the persona, or to import a real Firefox profile."
            }
            Self::Context => {
                "Use this to run several independent sessions in one browser, each with its own \
                 cookies."
            }
            Self::Session => {
                "Use this for work about the calls themselves rather than about the page."
            }
            Self::Intercept => {
                "Use this to change a request or a response before the page sees it."
            }
        }
    }
}

/// Ship state of a verb. `Preview` is listed and marked, never silently shipped
/// as measured behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// Contract is fixed; a change is a semver break.
    Stable,
    /// Shape may change. Marked in `--help`, docs, and the MCP description.
    Preview,
}

impl Stability {
    /// Docs / help label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preview => "preview",
        }
    }
}

/// Argument scalar type. One declaration drives the JSON Schema, the clap flag,
/// the HTTP decode, and the docs row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    /// UTF-8 string.
    Str,
    /// Signed integer.
    Int,
    /// Double.
    Float,
    /// Boolean. On the CLI this is a flag.
    Bool,
    /// Filesystem path, carried as a string.
    Path,
    /// One or more strings.
    StrList,
}

impl ArgType {
    /// JSON Schema type keyword.
    #[must_use]
    pub const fn json_type(self) -> &'static str {
        match self {
            Self::Str | Self::Path => "string",
            Self::Int => "integer",
            Self::Float => "number",
            Self::Bool => "boolean",
            Self::StrList => "array",
        }
    }
}

/// One argument of one verb.
#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    /// Argument name. Same token in JSON, on the CLI, and in the docs.
    pub name: &'static str,
    /// Scalar type.
    pub ty: ArgType,
    /// Required arguments are CLI positionals, in declaration order.
    pub required: bool,
    /// Default rendered into help and the schema. `None` means no default.
    pub default: Option<&'static str>,
    /// One line. Ends without a period only if it is a fragment.
    pub help: &'static str,
}

/// The element argument every verb that goes through the resolver accepts.
///
/// One spec rather than six copies, and the help line is what tells a face the
/// argument accepts the semantic forms: a selector argument whose help does not
/// name them is CSS only, like the frame verbs.
pub const SELECTOR_ARG: ArgSpec = ArgSpec {
    name: "selector",
    ty: ArgType::Str,
    required: true,
    default: None,
    help: "CSS, or a role:/text:/label:/placeholder:/testid:/ref: form.",
};

/// The deadline argument every verb that resolves an element accepts.
///
/// One spec rather than one per verb, so the name, the type and the help line
/// cannot drift between `click` and `fill`.
pub const TIMEOUT_ARG: ArgSpec = ArgSpec {
    name: "timeout_ms",
    ty: ArgType::Int,
    required: false,
    default: None,
    help: "Deadline for resolving the element. Default 10000, or LURIEN_TIMEOUT_MS.",
};

/// The deadline this call asked for, or the session default.
#[must_use]
pub fn timeout_ms(args: &Args) -> u64 {
    args.u64("timeout_ms", crate::locator::default_timeout_ms())
}

/// Shape a verb returns. Faces render it; a verb never formats for a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// Nothing but success.
    Empty,
    /// Human line.
    Text,
    /// Machine object.
    Json,
    /// Image bytes.
    Png,
}

impl OutputKind {
    /// One sentence naming what the caller gets back. Faces compose this into
    /// the tool description, so a shape change cannot leave the prose behind.
    #[must_use]
    pub const fn returns(self) -> &'static str {
        match self {
            Self::Empty => "Returns nothing: success, or a refusal that says what to do.",
            Self::Text => "Returns one line of text.",
            Self::Json => "Returns a JSON object, not prose.",
            Self::Png => "Returns PNG bytes, written to path when one is given.",
        }
    }
}

/// What a verb produced.
#[derive(Debug, Clone)]
pub enum Output {
    /// Success with no payload.
    Empty,
    /// Human-readable line.
    Text(String),
    /// Machine-readable object.
    Json(Value),
    /// PNG bytes.
    Png(Vec<u8>),
}

impl Output {
    /// Rendering for a text transport (CLI stdout, MCP text content).
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Empty => "ok".to_string(),
            Self::Text(s) => s.clone(),
            Self::Json(v) => serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()),
            Self::Png(b) => match crate::shot::png_size(b) {
                Some((w, h)) => format!("png {} bytes, {w}x{h}", b.len()),
                None => format!("png {} bytes", b.len()),
            },
        }
    }

    /// Rendering for a JSON transport (HTTP, structured MCP content).
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Empty => Value::Null,
            Self::Text(s) => Value::String(s.clone()),
            Self::Json(v) => v.clone(),
            Self::Png(b) => {
                let (width, height) = crate::shot::png_size(b).unwrap_or((0, 0));
                serde_json::json!({ "png_bytes": b.len(), "width": width, "height": height })
            }
        }
    }

    /// Raw image bytes, when this verb captured pixels.
    #[must_use]
    pub fn png(&self) -> Option<&[u8]> {
        match self {
            Self::Png(b) => Some(b),
            _ => None,
        }
    }
}

/// Boxed future a verb returns.
pub type VerbFuture<'a> = Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>>;

/// A verb body. Fn pointer, so a `VerbSpec` stays a `static` with no allocation
/// and no registration magic.
pub type VerbFn = for<'a> fn(&'a Session, &'a Args) -> VerbFuture<'a>;

/// Everything a face needs to expose a verb, and everything the docs need to
/// describe it.
pub struct VerbSpec {
    /// Canonical short name. This is the MCP tool name and the CLI subcommand.
    pub name: &'static str,
    /// Extra accepted names. Always includes the dotted `domain.name` form.
    pub aliases: &'static [&'static str],
    /// Owning domain.
    pub domain: Domain,
    /// One line, reused verbatim by `--help`, MCP `tools/list`, and the docs.
    pub summary: &'static str,
    /// Arguments, in CLI positional order for the required ones.
    pub args: &'static [ArgSpec],
    /// Output shape.
    pub output: OutputKind,
    /// Ship state.
    pub stability: Stability,
    /// Body.
    pub run: VerbFn,
}

impl VerbSpec {
    /// Look up one argument spec.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<&ArgSpec> {
        self.args.iter().find(|a| a.name == name)
    }

    /// Validate `args` against this spec, then run. Every face goes through
    /// here, so validation can never be face-specific.
    pub async fn call(&self, session: &Session, args: &Args) -> Result<Output, Error> {
        args.validate(self)?;
        (self.run)(session, args).await
    }
}

impl std::fmt::Debug for VerbSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerbSpec")
            .field("name", &self.name)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// Per-domain spec slices. A new domain is one line here.
const DOMAIN_SPECS: &[&[&VerbSpec]] = &[
    page::SPECS,
    dom::SPECS,
    input::SPECS,
    frame::SPECS,
    storage::SPECS,
    state::SPECS,
    net::SPECS,
    dialog::SPECS,
    observe::SPECS,
    profile::SPECS,
    context::SPECS,
    session::SPECS,
    intercept::SPECS,
];
/// Every verb, sorted by canonical name.
#[must_use]
pub fn registry() -> &'static [&'static VerbSpec] {
    static REGISTRY: OnceLock<Vec<&'static VerbSpec>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut all: Vec<&'static VerbSpec> = DOMAIN_SPECS.iter().flat_map(|d| d.iter().copied()).collect();
        all.sort_by_key(|s| s.name);
        all
    })
}

struct Index {
    by_name: BTreeMap<&'static str, &'static VerbSpec>,
    conflicts: Vec<String>,
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by_name: BTreeMap<&'static str, &'static VerbSpec> = BTreeMap::new();
        let mut conflicts = Vec::new();
        for spec in registry() {
            for name in std::iter::once(spec.name).chain(spec.aliases.iter().copied()) {
                if let Some(prev) = by_name.insert(name, spec) {
                    conflicts.push(format!(
                        "{name} claimed by both {} and {}",
                        prev.name, spec.name
                    ));
                }
            }
        }
        Index { by_name, conflicts }
    })
}

/// Resolve a canonical name or an alias.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static VerbSpec> {
    index().by_name.get(name).copied()
}

/// Names and aliases that two verbs both claim. A test asserts this is empty,
/// so a duplicate is caught without panicking at runtime.
#[must_use]
pub fn conflicts() -> &'static [String] {
    &index().conflicts
}

/// Every accepted token, canonical names and aliases alike.
#[must_use]
pub fn accepted_names() -> Vec<&'static str> {
    index().by_name.keys().copied().collect()
}

/// Verbs of one domain, in registry order.
#[must_use]
pub fn in_domain(domain: Domain) -> Vec<&'static VerbSpec> {
    registry()
        .iter()
        .copied()
        .filter(|s| s.domain == domain)
        .collect()
}
