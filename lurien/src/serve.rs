//! The HTTP face: many named sessions, one verb registry.
//!
//! This is the third transport over [`Session::call`], alongside the CLI and
//! MCP. It exists because an agent runtime needs browsers that outlive a single
//! process: each `browser_context_id` names a session that stays open across
//! requests, and different contexts run concurrently because a request clones its
//! session handle out from under a brief map lock and then drives that session
//! through its own mutex.
//!
//! The wire protocol is fixed by the clients already speaking it: `GET /v1/health`
//! and `POST /v1/browser/command`, with the legacy command names mapped onto verbs
//! by [`translate`]. The face never implements browser behavior; a legacy command
//! that has no verb is refused by name rather than reimplemented here.

use crate::error::Error;
use crate::launch::LaunchOptions;
use crate::resolve::resolve_engine;
use crate::session::Session;
use crate::verb::Args;
use runtime_foxdriver::ProxyConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Wire schema this face speaks. A client sending another version is refused.
pub const SCHEMA_VERSION: u32 = 1;

/// Backend name clients must send. There is one engine and no fallback.
pub const BACKEND: &str = "guise_foxdriver";

const MAX_REQUEST_BYTES: usize = 1 << 20;

/// Default bind address, overridden by `LURIEN_SERVE_BIND`.
pub const DEFAULT_BIND: &str = "127.0.0.1:7432";

/// One request from a client.
#[derive(Debug, Deserialize)]
pub struct Command {
    /// Wire schema version. Must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Legacy command name, resolved by [`translate`].
    pub command: String,
    /// Backend name. Must equal [`BACKEND`].
    pub backend: String,
    /// Session key. Sessions persist under this name across requests.
    #[serde(default)]
    pub browser_context_id: String,
    /// Caller-side program identifier, echoed in the lease id.
    #[serde(default)]
    pub program_id: Option<String>,
    /// Caller-side role, echoed in metadata.
    #[serde(default)]
    pub role: String,
    /// Caller-side identity, echoed in metadata.
    #[serde(default)]
    pub profile_id: String,
    /// Persistent Firefox profile directory for this session.
    #[serde(default)]
    pub profile_dir: Option<String>,
    /// Navigation target for `launch`, `resume`, and `goto`.
    #[serde(default)]
    pub url: Option<String>,
    /// Named proxy binding. `caido` resolves to the local Caido port.
    #[serde(default)]
    pub proxy_binding: Option<String>,
    /// Explicit proxy URL, or `caido`.
    #[serde(default)]
    pub proxy_url: Option<String>,
    /// Verb arguments, as strings. Decoded against the verb spec.
    #[serde(default)]
    pub args: Option<HashMap<String, String>>,
}

impl Command {
    /// One argument, trimmed, absent when empty.
    #[must_use]
    pub fn arg(&self, key: &str) -> Option<&str> {
        self.args
            .as_ref()?
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
    }

    /// First present argument among `keys`. Legacy clients spell the same thing
    /// several ways; the verb sees one name.
    fn any_arg(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|k| self.arg(k))
    }
}

/// One reply. Field order and the `skip_serializing_if` set are the wire
/// contract: a client reading `success`, `output`, `metadata`, `current_url`, or
/// `network_entries` keeps working unchanged.
#[derive(Debug, Default, Serialize)]
pub struct Reply {
    /// Whether the command completed.
    pub success: bool,
    /// Failure sentence. Empty on success.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Human-readable result.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub output: String,
    /// Structured result and posture.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
    /// Session this reply came from.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub browser_context_id: String,
    /// Lease identity, derived from program and context.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub browser_lease_id: String,
    /// URL at the end of the command.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub current_url: String,
    /// References to captured requests.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub request_refs: Vec<String>,
    /// Uncaught console errors seen during the command.
    pub console_error_count: usize,
    /// Redacted network rows, when the command carries traffic evidence.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub network_entries: Value,
}

impl Reply {
    /// Facts true of every reply from this binary.
    fn base_metadata() -> HashMap<String, Value> {
        HashMap::from([
            ("browser_backend".to_string(), json!(BACKEND)),
            (
                "browser_engine".to_string(),
                json!("runtime_foxdriver_firefox_bidi"),
            ),
            (
                "browser_runtime".to_string(),
                json!("runtime_foxdriver_firefox_bidi"),
            ),
            ("engine".to_string(), json!("firefox_bidi")),
            ("google_chrome_launch".to_string(), json!("forbidden")),
            ("standalone_browser_launch".to_string(), json!(false)),
        ])
    }

    /// Refusal. Never carries a partial result.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: message.into(),
            ..Self::default()
        }
    }

    /// Success for `command`, echoing the caller's identity fields.
    fn ok(command: &Command, current_url: String) -> Self {
        let mut metadata = Self::base_metadata();
        metadata.insert("profile_id".to_string(), json!(command.profile_id));
        metadata.insert("role".to_string(), json!(command.role));
        if let Some(dir) = command.profile_dir.as_deref() {
            metadata.insert("profile_dir".to_string(), json!(dir));
        }
        Self {
            success: true,
            metadata,
            browser_context_id: command.browser_context_id.trim().to_string(),
            browser_lease_id: lease_id(command),
            current_url,
            ..Self::default()
        }
    }

    /// Readiness posture. Answers even with no engine installed, reporting
    /// `stealth_engine=missing` and a warning rather than a healthy lie.
    #[must_use]
    pub fn health(active_contexts: usize) -> Self {
        Self::health_with_engine(active_contexts, resolve_engine().ok().as_deref())
    }

    /// The posture built from an already-resolved engine path, so the decision is
    /// testable without an installed engine.
    #[must_use]
    pub fn health_with_engine(active_contexts: usize, engine: Option<&str>) -> Self {
        let mut metadata = Self::base_metadata();
        // Capability handshake. A client gates readiness on this key, and it is
        // true because captcha handling lives in the engine, not in a verb.
        metadata.insert("captcha_solve".to_string(), json!(true));
        metadata.insert(
            "active_browser_contexts".to_string(),
            json!(active_contexts),
        );
        metadata.insert("schema_version".to_string(), json!(SCHEMA_VERSION));
        metadata.insert(
            "verbs".to_string(),
            json!(crate::verb::registry().len()),
        );
        let stealth = engine.is_some();
        metadata.insert(
            "stealth_engine".to_string(),
            json!(if stealth { "lurien" } else { "missing" }),
        );
        if let Some(path) = engine {
            metadata.insert("stealth_engine_path".to_string(), json!(path));
        }
        metadata.insert("webdriver_masked".to_string(), json!(stealth));
        metadata.insert("persona_coherence_gate".to_string(), json!("enforced"));
        if !stealth {
            metadata.insert(
                "warnings".to_string(),
                json!(["lurien engine not installed. Run install.sh or set LURIEN_BIN."]),
            );
        }
        Self {
            success: true,
            output: if stealth {
                "lurien serve healthy: stealth engine, persona coherence enforced".to_string()
            } else {
                "lurien serve UP but DEGRADED: lurien engine missing. see metadata.warnings"
                    .to_string()
            },
            metadata,
            ..Self::default()
        }
    }
}

/// Lease identity: stable per program and context, so a client can correlate
/// replies to the browser it leased.
fn lease_id(command: &Command) -> String {
    let program = command
        .program_id
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("lurien");
    let context = command.browser_context_id.trim();
    if context.is_empty() {
        String::new()
    } else {
        format!("{program}:{context}")
    }
}

/// Map a legacy command onto a registry verb and its arguments.
///
/// This table is the entire compatibility surface. Each arm names a verb that
/// exists in the registry, so a command cannot reach behavior the CLI and MCP
/// cannot also reach, and a command with no verb is refused instead of being
/// reimplemented here.
pub fn translate(command: &Command) -> Result<(&'static str, Args), String> {
    let name = command.command.trim();
    let mut args = Args::new();
    let verb = match name {
        "goto" => {
            let url = command
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .or_else(|| command.arg("url"))
                .ok_or("url is required")?;
            args.set("url", url);
            "goto"
        }
        "get_url" => "url",
        "dom_snapshot" | "dom_query" => "snapshot",
        "dom_extract" | "get_html" => "snapshot",
        "dom_click" => {
            let selector = selector_of(command)?;
            match command.arg("frame") {
                Some(frame) => {
                    args.set("frame", frame);
                    args.set("selector", selector);
                    "click-in"
                }
                None => {
                    args.set("selector", selector);
                    "click"
                }
            }
        }
        "dom_type" => {
            let selector = selector_of(command)?;
            let text = command.arg("text").unwrap_or("");
            match command.arg("frame") {
                Some(frame) => {
                    args.set("frame", frame);
                    args.set("selector", selector);
                    args.set("text", text);
                    "type-in"
                }
                None => {
                    args.set("selector", selector);
                    args.set("text", text);
                    "fill"
                }
            }
        }
        "dom_select" => {
            args.set("selector", selector_of(command)?);
            args.set(
                "value",
                command
                    .any_arg(&["value", "label"])
                    .ok_or("value is required for dom_select")?,
            );
            "select"
        }
        "dom_upload" => {
            args.set("selector", selector_of(command)?);
            args.set(
                "files",
                command
                    .any_arg(&["path", "file", "filepath"])
                    .ok_or("path is required for dom_upload")?,
            );
            "upload"
        }
        "dom_key" => {
            args.set("key", command.arg("key").ok_or("key is required")?);
            "press"
        }
        "dom_wait" => {
            if let Some(ms) = command.arg("ms").or_else(|| command.arg("timeout")) {
                args.set("ms", parse_i64(ms, "ms")?);
            }
            "wait"
        }
        "dom_screenshot" => "screenshot",
        "dom_readonly_eval" | "execute_js" => {
            args.set(
                "script",
                command
                    .any_arg(&["code", "script"])
                    .ok_or("code is required")?,
            );
            if let Some(frame) = command.arg("frame") {
                args.set("frame", frame);
            }
            "eval"
        }
        "dom_frames" => "frames",
        "dom_console" => "console",
        "dom_signals" => {
            if let Some(clear) = command.arg("clear") {
                args.set("clear", truthy(clear));
            }
            "signals"
        }
        "dom_downloads" | "dialog_list" => "dialogs",
        "dialog" => {
            args.set(
                "action",
                command
                    .any_arg(&["action", "op"])
                    .ok_or("action is required for dialog: accept or dismiss")?,
            );
            if let Some(text) = command.arg("text") {
                args.set("text", text);
            }
            if let Some(frame) = command.arg("frame") {
                args.set("frame", frame);
            }
            "dialog"
        }
        "mouse" => {
            args.set("x", parse_i64(command.arg("x").ok_or("x is required")?, "x")?);
            args.set("y", parse_i64(command.arg("y").ok_or("y is required")?, "y")?);
            "mouse"
        }
        "scroll" => {
            let (dx, dy) = scroll_delta(command)?;
            args.set("dx", dx);
            args.set("dy", dy);
            "scroll"
        }
        "get_cookies" | "dom_get_cookies" => "cookies",
        "set_cookie" | "dom_set_cookie" => {
            args.set("name", command.arg("name").ok_or("name is required")?);
            args.set("value", command.any_arg(&["value", "text"]).unwrap_or(""));
            args.set("domain", command.arg("domain").ok_or("domain is required")?);
            if let Some(path) = command.arg("path") {
                args.set("path", path);
            }
            if let Some(expires) = command.any_arg(&["expires", "expiry"]) {
                args.set("expires", parse_i64(expires, "expires")?);
            }
            if let Some(secure) = command.arg("secure") {
                args.set("secure", truthy(secure));
            }
            if let Some(http_only) = command.any_arg(&["http_only", "httponly"]) {
                args.set("http_only", truthy(http_only));
            }
            "set-cookie"
        }
        "dom_delete_cookie" => {
            args.set("name", command.arg("name").ok_or("name is required")?);
            "delete-cookie"
        }
        "dom_clear_cookies" => "clear-cookies",
        "get_state" | "dom_get_state" => "state",
        "set_state" | "dom_set_state" => {
            args.set(
                "snapshot",
                command.arg("snapshot").ok_or("snapshot is required")?,
            );
            "state-set"
        }
        "clear_state" | "dom_clear_state" => "state-clear",
        // Navigation aliases.
        "dom_back" => "back",
        "dom_forward" => "forward",
        "dom_reload" => "reload",
        "dom_stop" => "stop",
        "dom_get_title" => "title",
        "dom_get_source" => "snapshot",
        // Network log aliases.
        "dom_network_log" | "dom_network" => {
            if let Some(limit) = command.arg("limit") {
                args.set("limit", parse_i64(limit, "limit")?);
            }
            "net"
        }
        "dom_clear_network_log" | "dom_clear_network" => "net-clear",
        // Context management: list, create, switch, close.
        "dom_list_contexts" => "contexts",
        "dom_new_context" => {
            if let Some(url) = command.arg("url") {
                args.set("url", url);
            }
            "new-context"
        }
        "dom_switch_context" => {
            args.set("context_id", command.arg("context_id").ok_or("context_id is required")?);
            "switch-context"
        }
        "dom_close_context" => {
            args.set("context_id", command.arg("context_id").ok_or("context_id is required")?);
            "close-context"
        }
        // Local/session storage. Eval-based, no dedicated verb.
        "dom_get_local_storage" => {
            let key = command.arg("key").unwrap_or("");
            let script = if key.is_empty() {
                "JSON.stringify(localStorage)".to_string()
            } else {
                format!("localStorage.getItem({key:?})")
            };
            args.set("script", script);
            "eval"
        }
        "dom_set_local_storage" => {
            let key = command.arg("key").ok_or("key is required")?;
            let value = command.arg("value").unwrap_or("");
            args.set("script", format!("localStorage.setItem({key:?}, {value:?})"));
            "eval"
        }
        "dom_clear_local_storage" => {
            let key = command.arg("key");
            let script = match key {
                Some(k) => format!("localStorage.removeItem({k:?})"),
                None => "localStorage.clear()".to_string(),
            };
            args.set("script", script);
            "eval"
        }
        "dom_get_session_storage" => {
            let key = command.arg("key").unwrap_or("");
            let script = if key.is_empty() {
                "JSON.stringify(sessionStorage)".to_string()
            } else {
                format!("sessionStorage.getItem({key:?})")
            };
            args.set("script", script);
            "eval"
        }
        "dom_set_session_storage" => {
            let key = command.arg("key").ok_or("key is required")?;
            let value = command.arg("value").unwrap_or("");
            args.set("script", format!("sessionStorage.setItem({key:?}, {value:?})"));
            "eval"
        }
        "dom_clear_session_storage" => {
            let key = command.arg("key");
            let script = match key {
                Some(k) => format!("sessionStorage.removeItem({k:?})"),
                None => "sessionStorage.clear()".to_string(),
            };
            args.set("script", script);
            "eval"
        }
        // User agent and viewport: set via eval on the BiDi session.
        "dom_set_user_agent" => {
            let ua = command.arg("user_agent").ok_or("user_agent is required")?;
            args.set("script", format!("Object.defineProperty(navigator, 'userAgent', {{get: () => {ua:?}}})"));
            "eval"
        }
        "dom_set_viewport" => {
            let w = command.arg("width").ok_or("width is required")?;
            let h = command.arg("height").ok_or("height is required")?;
            args.set("script", format!("window.resizeTo({w}, {h})"));
            "eval"
        }
        "dom_set_extra_headers" => {
            // Extra headers are passed as JSON; stored in args for the session.
            let headers = command.arg("headers").unwrap_or("{}");
            args.set("headers", headers);
            "set-extra-headers"
        }
        // Request/response interception and header manipulation.
        "dom_get_headers" => "get-headers",
        "dom_set_header" => {
            args.set("name", command.arg("name").ok_or("name is required")?);
            args.set("value", command.arg("value").unwrap_or(""));
            "set-header"
        }
        "dom_delete_header" => {
            args.set("name", command.arg("name").ok_or("name is required")?);
            "delete-header"
        }
        "dom_intercept_request" => {
            args.set("pattern", command.arg("pattern").ok_or("pattern is required")?);
            if let Some(h) = command.arg("headers") { args.set("headers", h); }
            if let Some(b) = command.arg("body") { args.set("body", b); }
            "intercept-request"
        }
        "dom_intercept_response" => {
            args.set("pattern", command.arg("pattern").ok_or("pattern is required")?);
            if let Some(h) = command.arg("headers") { args.set("headers", h); }
            if let Some(b) = command.arg("body") { args.set("body", b); }
            "intercept-response"
        }
        "dom_clear_intercepts" => "clear-intercepts",
        // Captcha handling lives in the engine and runs during navigation, so
        // this command navigates (when a URL is given) and reports what the page
        // turned out to be. There is no solver to call.
        "captcha_solve" => {
            match command
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .or_else(|| command.arg("url"))
            {
                Some(url) => {
                    args.set("url", url);
                    "goto"
                }
                None => "snapshot",
            }
        }
        other => {
            return Err(format!(
                "unsupported command: {other}. run `lurien verbs` for the verb surface"
            ))
        }
    };
    Ok((verb, args))
}

/// Selector for an element command: an explicit selector, or an element ref from
/// a prior snapshot.
fn selector_of(command: &Command) -> Result<String, String> {
    if let Some(reference) = command.arg("ref") {
        return Ok(selector_from_ref(reference));
    }
    command
        .arg("selector")
        .map(str::to_string)
        .ok_or_else(|| "selector or ref is required".to_string())
}

/// `element:<n>` and `#id` refs from a snapshot become a CSS selector; anything
/// else is already one.
fn selector_from_ref(reference: &str) -> String {
    match reference.split_once(':') {
        Some(("element" | "ref", rest)) => format!("[data-lurien-ref=\"{}\"]", rest.trim()),
        _ => reference.to_string(),
    }
}

/// Scroll accepts explicit deltas or a direction plus an amount.
fn scroll_delta(command: &Command) -> Result<(i64, i64), String> {
    if let (Some(dx), Some(dy)) = (command.arg("dx"), command.arg("dy")) {
        return Ok((parse_i64(dx, "dx")?, parse_i64(dy, "dy")?));
    }
    let amount = match command.arg("amount") {
        Some(raw) => parse_i64(raw, "amount")?,
        None => 400,
    };
    match command.arg("direction").unwrap_or("down") {
        "down" => Ok((0, amount)),
        "up" => Ok((0, -amount)),
        "right" => Ok((amount, 0)),
        "left" => Ok((-amount, 0)),
        other => Err(format!(
            "direction must be up, down, left, or right, got {other:?}"
        )),
    }
}

fn parse_i64(raw: &str, field: &str) -> Result<i64, String> {
    raw.trim()
        .parse::<i64>()
        .map_err(|e| format!("{field} must be an integer: {e}"))
}

fn truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Proxy for a session. `caido` names the local intercepting proxy so a client
/// does not have to know its port.
fn proxy_of(command: &Command) -> Result<Option<ProxyConfig>, String> {
    let url = command.proxy_url.as_deref().unwrap_or("").trim();
    let binding = command.proxy_binding.as_deref().unwrap_or("").trim();
    let caido = || {
        let port = std::env::var("CAIDO_PORT").unwrap_or_else(|_| "8080".to_string());
        ProxyConfig::from_url(&format!("http://127.0.0.1:{port}"))
            .map(Some)
            .map_err(|e| format!("parse caido proxy config: {e}"))
    };
    if url.is_empty() || url.eq_ignore_ascii_case("none") {
        if binding.eq_ignore_ascii_case("caido") {
            return caido();
        }
        return Ok(None);
    }
    if url.eq_ignore_ascii_case("caido") {
        return caido();
    }
    ProxyConfig::from_url(url)
        .map(Some)
        .map_err(|e| format!("parse proxy config {url}: {e}"))
}

/// Whether this command asks for a headful window. Per-command `args["headful"]`
/// wins, else the process-wide `LURIEN_SERVE_HEADFUL`, else headless so an
/// unattended run needs no display.
fn headless_of(command: &Command) -> bool {
    let attended = match command.arg("headful") {
        Some(raw) => truthy(raw),
        None => std::env::var("LURIEN_SERVE_HEADFUL")
            .map(|v| truthy(&v))
            .unwrap_or(false),
    };
    !attended
}

/// Named sessions. A request clones its session handle out under a brief map
/// lock and then drives it, so different contexts run concurrently; the
/// per-context launch lock stops two requests from spawning two engines for one
/// context without blocking any other context.
#[derive(Default)]
pub struct Registry {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    launch_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Registry {
    /// Live session for `context`, if open.
    pub async fn get(&self, context: &str) -> Option<Arc<Session>> {
        self.sessions.lock().await.get(context).cloned()
    }

    /// Open context names.
    pub async fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.sessions.lock().await.keys().cloned().collect();
        names.sort();
        names
    }

    /// Number of open contexts.
    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// True when no session is open.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    async fn launch_lock(&self, context: &str) -> Arc<Mutex<()>> {
        Arc::clone(
            self.launch_locks
                .lock()
                .await
                .entry(context.to_string())
                .or_default(),
        )
    }

    /// The session for `context`, creating it on first use. Sessions launch their
    /// engine lazily, so this is cheap until a verb needs a page.
    pub async fn open(&self, command: &Command) -> Result<Arc<Session>, String> {
        let context = command.browser_context_id.trim();
        if context.is_empty() {
            return Err("browser_context_id is required".to_string());
        }
        if let Some(session) = self.get(context).await {
            return Ok(session);
        }
        let lock = self.launch_lock(context).await;
        let _guard = lock.lock().await;
        if let Some(session) = self.get(context).await {
            return Ok(session);
        }
        let session = Arc::new(Session::with_options(LaunchOptions {
            headless: headless_of(command),
            profile_dir: command
                .profile_dir
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            proxy: proxy_of(command)?,
            ..LaunchOptions::default()
        }));
        self.sessions
            .lock()
            .await
            .insert(context.to_string(), Arc::clone(&session));
        Ok(session)
    }

    /// Close and forget `context`. Returns whether a session existed. The launch
    /// lock is dropped too, so a process that churns contexts does not grow one
    /// entry per closed context.
    pub async fn close(&self, context: &str) -> Result<bool, Error> {
        let taken = self.sessions.lock().await.remove(context);
        self.launch_locks.lock().await.remove(context);
        match taken {
            Some(session) => {
                let result = session.close().await;
                result.map(|()| true)
            }
            None => Ok(false),
        }
    }
}

/// Run one command against the registry.
pub async fn dispatch(command: Command, registry: &Registry) -> Reply {
    if command.schema_version != SCHEMA_VERSION {
        return Reply::err(format!(
            "unsupported schema_version {}; expected {SCHEMA_VERSION}",
            command.schema_version
        ));
    }
    if command.backend.trim() != BACKEND {
        return Reply::err(format!(
            "unsupported backend {}; expected {BACKEND}",
            command.backend
        ));
    }
    match command.command.trim() {
        "launch" | "resume" | "launch_or_resume" => open_or_resume(&command, registry).await,
        "close" => close_context(&command, registry).await,
        "list_contexts" => {
            let contexts = registry.list().await;
            let mut reply = Reply {
                success: true,
                output: format!("open browser contexts: {}", contexts.len()),
                metadata: Reply::base_metadata(),
                ..Reply::default()
            };
            reply.metadata.insert("count".to_string(), json!(contexts.len()));
            reply.metadata.insert("contexts".to_string(), json!(contexts));
            reply
        }
        _ => run_verb(&command, registry).await,
    }
}

/// `launch` and `resume` are the same operation: get the session for this
/// context, navigate when a URL was given, and report where it landed.
async fn open_or_resume(command: &Command, registry: &Registry) -> Reply {
    let session = match registry.open(command).await {
        Ok(session) => session,
        Err(err) => return Reply::err(err),
    };
    let url = command
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .or_else(|| command.arg("url"));
    if let Some(url) = url {
        let mut args = Args::new();
        args.set("url", url);
        if let Err(err) = session.call("goto", &args).await {
            return Reply::err(format!("navigate failed: {err}"));
        }
    }
    let mut reply = Reply::ok(command, session.current_url().await);
    reply.output = format!(
        "lurien {} completed for role={} context={}",
        command.command.trim(),
        command.role,
        command.browser_context_id.trim()
    );
    attach_network(&mut reply, &session, 25).await;
    reply
}

async fn close_context(command: &Command, registry: &Registry) -> Reply {
    let context = command.browser_context_id.trim();
    if context.is_empty() {
        return Reply::err("browser_context_id is required");
    }
    let existed = match registry.close(context).await {
        Ok(existed) => existed,
        Err(err) => return Reply::err(format!("close failed: {err}")),
    };
    let mut reply = Reply::ok(command, String::new());
    reply.output = if existed {
        format!("lurien closed context={context}")
    } else {
        format!("lurien context={context} was not open")
    };
    reply.metadata.insert("closed".to_string(), json!(existed));
    reply
}

/// Translate a legacy command to a verb and run it. Nothing here knows what any
/// verb does.
async fn run_verb(command: &Command, registry: &Registry) -> Reply {
    let (verb, args) = match translate(command) {
        Ok(pair) => pair,
        Err(err) => return Reply::err(err),
    };
    let context = command.browser_context_id.trim();
    let Some(session) = registry.get(context).await else {
        return Reply::err(
            "browser context is not open; send command=launch or command=resume first",
        );
    };
    let output = match session.call(verb, &args).await {
        Ok(output) => output,
        Err(err) => return Reply::err(format!("{verb}: {err}")),
    };
    let mut reply = Reply::ok(command, session.current_url().await);
    reply.output = output.to_text();
    reply.metadata.insert("verb".to_string(), json!(verb));
    reply
        .metadata
        .insert("command".to_string(), json!(command.command.trim()));
    let value = output.to_json();
    if !value.is_null() {
        reply.metadata.insert("result".to_string(), value);
    }
    if let Some(png) = output.png() {
        reply
            .metadata
            .insert("size_bytes".to_string(), json!(png.len()));
        reply.metadata.insert("format".to_string(), json!("png"));
    }
    // The count comes from the verb that already read the sensor grid. Reading
    // it again on every command would cost a page round trip per request.
    reply.console_error_count = output
        .to_json()
        .get("uncaught_errors")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    attach_network(&mut reply, &session, 25).await;
    reply
}

/// Redacted traffic evidence for a reply. Taken by calling the `net` verb, so
/// this face redacts nothing itself and can never disagree with what the CLI and
/// MCP show for the same session.
async fn attach_network(reply: &mut Reply, session: &Session, limit: usize) {
    let mut args = Args::new();
    args.set("limit", limit as i64);
    let Ok(output) = session.call("net", &args).await else {
        return;
    };
    let value = output.to_json();
    let Some(rows) = value.get("entries").and_then(Value::as_array) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    reply.request_refs = rows
        .iter()
        .filter_map(|row| row.get("ref").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    reply
        .metadata
        .insert("network_entry_count".to_string(), json!(rows.len()));
    reply.network_entries = json!(rows);
}

/// Serve until the process is stopped. Binds `LURIEN_SERVE_BIND` (default
/// [`DEFAULT_BIND`]) and refuses to start without an engine, because every
/// session this face hands out needs one.
pub async fn run(bind: Option<&str>) -> Result<(), Error> {
    let bind = match bind {
        Some(b) => b.to_string(),
        None => std::env::var("LURIEN_SERVE_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string()),
    };
    let engine = crate::resolve::resolve_engine_checked()?;
    let listener = TcpListener::bind(&bind)
        .await
        .map_err(|e| Error::Other(format!("bind {bind}: {e}")))?;
    eprintln!("lurien serve listening on http://{bind}");
    eprintln!("lurien serve engine: {engine}");
    let registry = Arc::new(Registry::default());
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| Error::Other(format!("accept: {e}")))?;
        // One task per connection so commands to different sessions overlap.
        let registry = Arc::clone(&registry);
        tokio::spawn(handle_connection(stream, registry));
    }
}

async fn handle_connection(mut stream: TcpStream, registry: Arc<Registry>) {
    let (status, reply) = match read_request(&mut stream).await {
        Ok((method, raw_path, body)) => route(method.as_str(), &raw_path, &body, &registry).await,
        Err(err) => (400, Reply::err(err)),
    };
    write_response(&mut stream, status, &reply).await;
}

/// Route one request. Pure apart from the registry, so the protocol is testable
/// without a socket.
pub async fn route(
    method: &str,
    raw_path: &str,
    body: &[u8],
    registry: &Registry,
) -> (u16, Reply) {
    let path_without_query = raw_path.split('?').next().unwrap_or(raw_path);
    let path = if path_without_query.len() > 1 {
        path_without_query.trim_end_matches('/')
    } else {
        path_without_query
    };
    match (method, path) {
        ("GET", "/health" | "/v1/health") => (200, Reply::health(registry.len().await)),
        ("POST", "/v1/browser/command") => match serde_json::from_slice::<Command>(body) {
            Ok(command) => (200, dispatch(command, registry).await),
            Err(err) => (400, Reply::err(format!("invalid JSON command: {err}"))),
        },
        _ => (404, Reply::err("not found")),
    }
}

/// Read one HTTP request: method, path, and exactly `Content-Length` bytes.
async fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err("request headers too large".to_string());
        }
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|e| format!("read request: {e}"))?;
        if read == 0 {
            return Err("connection closed before request headers ended".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let (method, path) = request_line(&headers)?;
    let length = content_length(&headers)?.unwrap_or(0);
    if length > MAX_REQUEST_BYTES {
        return Err(format!("request body too large: {length} bytes"));
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|e| format!("read body: {e}"))?;
        if read == 0 {
            return Err(format!(
                "connection closed after {} of {length} body bytes",
                body.len()
            ));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok((method, path, body))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Method and path from the request line, skipping leading blank lines.
pub fn request_line(headers: &str) -> Result<(String, String), String> {
    let line = headers
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or("empty request")?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("missing method")?.to_string();
    let path = parts.next().ok_or("missing path")?.to_string();
    Ok((method, path))
}

/// `Content-Length`, refusing conflicting duplicates rather than guessing which
/// one bounds the body.
pub fn content_length(headers: &str) -> Result<Option<usize>, String> {
    let mut found: Option<usize> = None;
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        let parsed = value
            .trim()
            .parse::<usize>()
            .map_err(|e| format!("invalid Content-Length: {e}"))?;
        match found {
            Some(existing) if existing != parsed => {
                return Err(format!(
                    "conflicting Content-Length headers: {existing} vs {parsed}"
                ))
            }
            _ => found = Some(parsed),
        }
    }
    Ok(found)
}

async fn write_response(stream: &mut TcpStream, status: u16, reply: &Reply) {
    let body = serde_json::to_vec(reply).unwrap_or_else(|_| b"{\"success\":false}".to_vec());
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(&body).await;
    let _ = stream.flush().await;
}
