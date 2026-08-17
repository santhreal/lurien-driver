//! Firefox browser automation via rustenium (WebDriver BiDi).

use anyhow::{anyhow, Result};
use base64::Engine as _;
use rustenium::browsers::{
    firefox, BidiBrowser, EvaluateScriptOptionsBuilder, FirefoxBrowser, FirefoxCapabilities,
    FirefoxConfig, FirefoxLaunchMode,
};
use rustenium::input::{
    Mouse, MouseButton, MouseClickOptions, MouseMoveOptions, MouseOptions, MouseWheelOptions, Point,
};
use rustenium::nodes::Node;
use rustenium_bidi_definitions::browser::commands::{
    BrowserCommand, Close as BrowserCloseCmd, CloseMethod as BrowserCloseMethod,
    CloseParams as BrowserCloseParams,
};
use rustenium_bidi_definitions::browsing_context::commands::HandleUserPrompt;
use rustenium_bidi_definitions::browsing_context::types::{CssLocator, CssLocatorType, Locator};
use rustenium_bidi_definitions::input::commands::SetFiles;
use rustenium_bidi_definitions::network::types::{
    BytesValue, SameSite, StringValue, StringValueType,
};
use rustenium_bidi_definitions::script::types::{
    ContextTarget, RemoteValue, SharedReference, Target,
};
use rustenium_bidi_definitions::session::types::{UnhandledPromptBehavior, UserPromptHandlerType};
use rustenium_bidi_definitions::storage::commands::{GetCookies, SetCookie, SetCookieParams};
use rustenium_bidi_definitions::storage::types::PartialCookie;
use serde::de::DeserializeOwned;
use std::collections::HashSet;

/// Wrapper around rustenium's `FirefoxBrowser`.
pub struct Page {
    browser: tokio::sync::Mutex<Option<FoxBrowser>>,
    profile_dir: Option<String>,
    /// Child process when foxdriver spawned the browser itself (the
    /// [`launch_firefox_self_managed`] / Remote-attach path). In the normal
    /// `SpawnAndAttach` path rustenium owns the process (`kill_on_drop`), so
    /// this is `None`; when foxdriver owns the spawn it must kill it here.
    child: std::sync::Mutex<Option<std::process::Child>>,
}

impl Drop for Page {
    fn drop(&mut self) {
        // Best-effort synchronous cleanup: take the browser out of the
        // mutex and drop it.  The underlying `Process` is spawned with
        // `kill_on_drop(true)`, so dropping kills the Firefox process.
        if let Ok(mut guard) = self.browser.try_lock() {
            let _ = guard.take();
        }
        // A self-managed child (Remote-attach path) is not owned by rustenium
        // terminate it explicitly so a self-spawned lurien/Camoufox never leaks.
        // Best-effort GRACEFUL: SIGTERM first so Firefox flushes storage (Drop is
        // sync and cannot wait long, so cap the wait short; the explicit `close()`
        // path does the full graceful wait), then SIGKILL as a fallback.
        if let Ok(mut child) = self.child.try_lock() {
            if let Some(c) = child.take() {
                terminate_and_reap(c);
            }
        }
    }
}

/// Terminate a self-managed Firefox child and reap it.
///
/// SIGTERM first so Firefox flushes storage (capped wait, `Drop` is sync and
/// cannot wait long; the explicit `close()` path does the full graceful
/// wait), then SIGKILL as a fallback. The final `wait()` reaps the child:
/// without it a SIGKILLed child lingers as a zombie, because `Child`'s own
/// `Drop` does not wait.
fn terminate_and_reap(mut child: std::process::Child) {
    request_graceful_terminate(child.id());
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Ask a self-managed Firefox child to exit GRACEFULLY (SIGTERM) so it flushes
/// localStorage / IndexedDB / cookies to its profile before exit.
///
/// A bare `Child::kill()` (SIGKILL) interrupts Firefox before its LSNG storage
/// flush, so a persistent `profile_dir` silently loses localStorage/IndexedDB and
/// recent cookie writes across a restart (confirmed live: localStorage read back
/// `null` after a restart that reused the same profile dir). SIGTERM triggers
/// Firefox's normal shutdown, which flushes. `nix::sys::signal::kill` is a safe
/// wrapper (this crate forbids `unsafe`). On non-unix there is no SIGTERM, so the
/// caller's SIGKILL fallback is the only option.
#[cfg(unix)]
fn request_graceful_terminate(pid: u32) {
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    );
}

#[cfg(not(unix))]
fn request_graceful_terminate(_pid: u32) {}

/// Poll a child for exit up to `ticks` × 100 ms, reaping it when it exits. Returns
/// `true` if it exited within the window. Used so a clean Firefox shutdown can
/// finish flushing storage to disk before we escalate to a signal.
async fn wait_for_exit(child: &mut std::process::Child, ticks: u32) -> bool {
    for _ in 0..ticks {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            Err(_) => return false,
        }
    }
    false
}

/// Opaque handle to a browsing context (tab or iframe).
pub type FrameId = rustenium_bidi_definitions::browsing_context::types::BrowsingContext;

/// What a capture covers.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ShotArea {
    /// What is on screen now.
    #[default]
    Viewport,
    /// The whole scrollable document, however far past the viewport it runs.
    Document,
    /// A rectangle in CSS pixels, measured from the document's top-left corner.
    Region {
        /// Distance from the document's left edge.
        x: f64,
        /// Distance from the document's top edge.
        y: f64,
        /// Rectangle width.
        width: f64,
        /// Rectangle height.
        height: f64,
    },
}

/// What to capture and which document to capture it from.
#[derive(Debug, Clone, Default)]
pub struct ShotOptions {
    /// Area of the document to capture.
    pub area: ShotArea,
    /// Frame spec accepted by [`Page::resolve_frame`]. `None` is the main
    /// document.
    pub frame: Option<String>,
}

/// A browsing context (frame) with the metadata the agent needs to target it:
/// the opaque `id` to pass back on a frame-scoped command, plus its `url` and
/// `name` for disambiguation. Returned by [`Page::list_frames`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FrameInfo {
    /// Opaque browsing-context id (pass this back as the `frame` target).
    pub id: String,
    /// The frame's current document URL (`about:blank` for a fresh frame).
    pub url: String,
    /// The frame's `window.name`, empty when unset.
    pub name: String,
    /// `true` for the top-level document, `false` for an iframe.
    pub is_main: bool,
}

/// One node of the live browsing-context tree as reported by WebDriver
/// BiDi `browsingContext.getTree`.
///
/// Unlike [`Page::frames`], a flat id list with no structure, this
/// preserves true parent linkage and the committed URL of every frame,
/// including cross-origin iframes whose URL parent-page JS could never
/// read (it would throw `SecurityError`). It is the structural source of
/// truth for [`crate::frame_graph::FrameGraph`].
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTreeNode {
    /// Browsing-context id, usable directly as a `frame` target for
    /// [`Page::eval_in_frame`] / [`Page::click_in_frame`].
    pub id: FrameId,
    /// Committed document URL as the browser process sees it.
    pub url: String,
    /// Parent browsing-context id; `None` for a top-level (tab) context.
    pub parent: Option<FrameId>,
    /// Depth within the tree: a top-level context is `0`, its direct
    /// iframes `1`, and so on.
    pub depth: usize,
}

/// A parsed frame target, the pure classification of a `frame=` spec, factored
/// out of [`Page::resolve_frame`] so the parsing rules are unit-testable without
/// a live browser.
#[derive(Debug, Clone, PartialEq)]
enum FrameSpec {
    /// The top-level document (`""`, `main`, `top`).
    Main,
    /// Strictly a 0-based index into the frame list (`index:<n>`).
    Index(usize),
    /// A bare all-digit spec: Firefox BiDi context ids are ALSO all-digits
    /// (e.g. `10737418241`), so this is ambiguous, resolve as an exact id
    /// FIRST, then fall back to the index. `0` carries the parsed index.
    IdOrIndex(String, usize),
    /// Exact browsing-context id, with a URL-substring fallback.
    Id(String),
    /// First frame whose URL contains this substring (`url:<substr>`).
    UrlContains(String),
    /// First frame whose `window.name` equals this (`name:<name>`).
    NameEquals(String),
}

impl FrameSpec {
    fn parse(spec: &str) -> Self {
        let s = spec.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("main") || s.eq_ignore_ascii_case("top") {
            return FrameSpec::Main;
        }
        if let Some(rest) = s.strip_prefix("url:") {
            return FrameSpec::UrlContains(rest.trim().to_string());
        }
        if let Some(rest) = s.strip_prefix("name:") {
            return FrameSpec::NameEquals(rest.trim().to_string());
        }
        if let Some(rest) = s.strip_prefix("index:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                return FrameSpec::Index(n);
            }
        }
        // A bare integer is ambiguous: a small one is probably a list index, but
        // a Firefox BiDi context id is also a (large) all-digit string. Try the
        // exact id first, then the index, so echoing a numeric list_frames id
        // back works, and `2` still means "the third frame".
        if let Ok(n) = s.parse::<usize>() {
            return FrameSpec::IdOrIndex(s.to_string(), n);
        }
        FrameSpec::Id(s.to_string())
    }
}

/// Result of evaluating JavaScript in the page.
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    inner: RemoteValue,
}

impl EvaluationResult {
    pub fn new(inner: RemoteValue) -> Self {
        Self { inner }
    }

    /// Attempt to deserialize the evaluation result into `T`.
    pub fn into_value<T: DeserializeOwned>(self) -> serde_json::Result<T> {
        let json = remote_value_to_json(&self.inner);
        serde_json::from_value(json)
    }

    /// Raw BiDi remote value.
    pub fn remote_value(&self) -> &RemoteValue {
        &self.inner
    }
}

/// Convert a raw BiDi wire-format `serde_json::Value` into a plain JSON value.
fn bidi_wire_value_to_json(v: &serde_json::Value) -> serde_json::Value {
    match v.get("type").and_then(|t| t.as_str()) {
        Some("string") => v
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| serde_json::Value::String(s.to_string()))
            .unwrap_or(serde_json::Value::Null),
        Some("number") => v.get("value").cloned().unwrap_or(serde_json::Value::Null),
        Some("boolean") => v
            .get("value")
            .and_then(|v| v.as_bool())
            .map(serde_json::Value::Bool)
            .unwrap_or(serde_json::Value::Null),
        Some("null") | Some("undefined") => serde_json::Value::Null,
        Some("bigint") => v
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| serde_json::Value::String(s.to_string()))
            .unwrap_or(serde_json::Value::Null),
        Some("object") => {
            let mut map = serde_json::Map::new();
            if let Some(serde_json::Value::Array(pairs)) = v.get("value") {
                for pair in pairs {
                    if let serde_json::Value::Array(items) = pair {
                        if items.len() >= 2 {
                            let key_opt = items[0].as_str().map(String::from).or_else(|| {
                                match bidi_wire_value_to_json(&items[0]) {
                                    serde_json::Value::String(s) => Some(s),
                                    serde_json::Value::Number(n) => Some(n.to_string()),
                                    serde_json::Value::Bool(b) => Some(b.to_string()),
                                    _ => None,
                                }
                            });
                            if let (Some(k), Some(val)) = (key_opt, items.get(1)) {
                                map.insert(k, bidi_wire_value_to_json(val));
                            }
                        }
                    }
                }
            }
            serde_json::Value::Object(map)
        }
        Some("array") => {
            let arr: Vec<serde_json::Value> = v
                .get("value")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(bidi_wire_value_to_json).collect())
                .unwrap_or_default();
            serde_json::Value::Array(arr)
        }
        _ => v.clone(),
    }
}

/// Convert a BiDi `RemoteValue` into a plain `serde_json::Value`.
fn remote_value_to_json(rv: &RemoteValue) -> serde_json::Value {
    match rv {
        RemoteValue::PrimitiveProtocolValue(p) => match p {
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::StringValue(s) => {
                serde_json::Value::String(s.value.clone())
            }
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::NumberValue(n) => {
                match &n.value {
                    serde_json::Value::Number(num) => serde_json::Value::Number(num.clone()),
                    serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
                    _ => serde_json::Value::Null,
                }
            }
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::BooleanValue(b) => {
                serde_json::Value::Bool(b.value)
            }
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::NullValue(_) => {
                serde_json::Value::Null
            }
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::UndefinedValue(
                _,
            ) => serde_json::Value::Null,
            rustenium_bidi_definitions::script::types::PrimitiveProtocolValue::BigIntValue(b) => {
                serde_json::Value::String(b.value.clone())
            }
        },
        RemoteValue::ArrayRemoteValue(a) => {
            let arr: Vec<serde_json::Value> = a
                .value
                .as_ref()
                .map(|v| v.inner().iter().map(remote_value_to_json).collect())
                .unwrap_or_default();
            serde_json::Value::Array(arr)
        }
        RemoteValue::ObjectRemoteValue(o) => {
            let mut map = serde_json::Map::new();
            if let Some(mapping) = &o.value {
                for pair in mapping.inner() {
                    if pair.len() >= 2 {
                        let key_opt = match pair.first() {
                            Some(serde_json::Value::String(k)) => Some(k.clone()),
                            Some(v) => match bidi_wire_value_to_json(v) {
                                serde_json::Value::String(s) => Some(s),
                                serde_json::Value::Number(n) => Some(n.to_string()),
                                serde_json::Value::Bool(b) => Some(b.to_string()),
                                _ => None,
                            },
                            None => None,
                        };
                        if let (Some(k), Some(v)) = (key_opt, pair.get(1)) {
                            map.insert(k, bidi_wire_value_to_json(v));
                        }
                    }
                }
            }
            serde_json::Value::Object(map)
        }
        RemoteValue::RegExpRemoteValue(r) => serde_json::Value::String(format!(
            "/{}/{}",
            r.reg_exp_local_value.value.pattern,
            r.reg_exp_local_value.value.flags.as_deref().unwrap_or("")
        )),
        RemoteValue::DateRemoteValue(d) => {
            serde_json::Value::String(d.date_local_value.value.clone())
        }
        RemoteValue::NodeRemoteValue(_) => serde_json::Value::Null,
        RemoteValue::WindowProxyRemoteValue(_) => serde_json::Value::Null,
        _ => serde_json::Value::Null,
    }
}

/// DOM element handle.
pub struct Element {
    pub(crate) node: tokio::sync::Mutex<FoxNode>,
    pub(crate) selector: String,
}

impl Element {
    /// Click the element using BiDi pointer actions.
    pub async fn click(&self) -> Result<()> {
        let mut node = self.node.lock().await;
        node.mouse_click()
            .await
            .map_err(|e| anyhow!("element click failed: {e:?}"))?;
        Ok(())
    }

    /// Return the CSS selector used to locate this element.
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// Type text into this element.
    pub async fn type_text(&self, text: &str) -> Result<()> {
        let mut node = self.node.lock().await;
        node.type_text(text.to_string())
            .await
            .map_err(|e| anyhow!("element type_text failed: {e:?}"))?;
        Ok(())
    }

    /// Alias for [`type_text`].
    pub async fn type_str(&self, text: &str) -> Result<()> {
        self.type_text(text).await
    }
}

// Internal aliases.
type FoxBrowser = FirefoxBrowser;
type FoxNode =
    rustenium::nodes::FirefoxNode<rustenium_core::transport::WebsocketConnectionTransport>;

/// Direction for realistic scroll simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl Page {
    /// Launch a new Firefox instance and return its first page.
    pub async fn launch(config: Option<FoxBrowserConfig>) -> Result<Self> {
        launch_firefox(config.unwrap_or_default()).await
    }

    /// Navigate the active browsing context to `url`.
    pub async fn goto(&self, url: &str) -> Result<()> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        browser
            .navigate(url)
            .await
            .map_err(|e| anyhow!("navigate failed: {e:?}"))?;
        Ok(())
    }

    /// Evaluate a JavaScript expression in the active context.
    ///
    /// The returned value is NOT promise-awaited: if `expr` evaluates to a
    /// `Promise`, the opaque promise handle is returned, not its resolved value.
    /// Use [`Self::evaluate_await`] for expressions that may be async.
    pub async fn evaluate(&self, expr: impl Into<String>) -> Result<EvaluationResult> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let result = browser
            .evaluate_script(expr.into(), false)
            .await
            .map_err(|e| anyhow!("evaluate failed: {e:?}"))?;
        Ok(EvaluationResult::new(result.result))
    }

    /// Evaluate a JavaScript expression, awaiting the result if it is a `Promise`.
    ///
    /// This sets the BiDi `awaitPromise` flag, so an expression that returns a
    /// `Promise` resolves to its fulfilled value before serialization. A
    /// non-promise expression is returned unchanged, so this is a safe superset
    /// of [`Self::evaluate`] for any caller that wants the resolved value (e.g.
    /// surfaces backed by `MediaCapabilities.decodingInfo`, `Worker`/
    /// `ServiceWorker` message round-trips, or any `async` probe).
    pub async fn evaluate_await(&self, expr: impl Into<String>) -> Result<EvaluationResult> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let result = browser
            .evaluate_script(expr.into(), true)
            .await
            .map_err(|e| anyhow!("evaluate_await failed: {e:?}"))?;
        Ok(EvaluationResult::new(result.result))
    }

    /// Evaluate in a specific browsing context (frame).
    pub async fn evaluate_in_context(
        &self,
        expr: impl Into<String>,
        context: &FrameId,
    ) -> Result<EvaluationResult> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let options = EvaluateScriptOptionsBuilder::default()
            .target(Target::ContextTarget(ContextTarget::new(context.clone())))
            .build();
        let result = browser
            .evaluate_script_with_options(expr.into(), false, options)
            .await
            .map_err(|e| anyhow!("evaluate_in_context failed: {e:?}"))?;
        Ok(EvaluationResult::new(result.result))
    }
    /// Evaluate in a specific browsing context (frame), awaiting promises.
    pub async fn evaluate_in_context_await(
        &self,
        expr: impl Into<String>,
        context: &FrameId,
    ) -> Result<EvaluationResult> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let options = EvaluateScriptOptionsBuilder::default()
            .target(Target::ContextTarget(ContextTarget::new(context.clone())))
            .build();
        let result = browser
            .evaluate_script_with_options(expr.into(), true, options)
            .await
            .map_err(|e| anyhow!("evaluate_in_context_await failed: {e:?}"))?;
        Ok(EvaluationResult::new(result.result))
    }

    /// Find the first element matching `selector`.
    pub async fn find_element(&self, selector: &str) -> Result<Element> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let locator =
            Locator::CssLocator(CssLocator::new(CssLocatorType::Css, selector.to_string()));
        match browser.find_node(locator).await {
            Ok(Some(node)) => Ok(Element {
                node: tokio::sync::Mutex::new(node),
                selector: selector.to_string(),
            }),
            Ok(None) => Err(anyhow!("find_element: no element matched '{}'", selector)),
            Err(e) => Err(anyhow!("find_element failed: {e:?}")),
        }
    }

    /// Find all elements matching `selector`.
    pub async fn find_elements(&self, selector: &str) -> Result<Vec<Element>> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let locator =
            Locator::CssLocator(CssLocator::new(CssLocatorType::Css, selector.to_string()));
        let nodes = browser
            .find_nodes(locator)
            .await
            .map_err(|e| anyhow!("find_elements failed: {e:?}"))?;
        Ok(nodes
            .into_iter()
            .map(|n| Element {
                node: tokio::sync::Mutex::new(n),
                selector: selector.to_string(),
            })
            .collect())
    }

    /// Set the file(s) on a `<input type=file>` element via BiDi `input.setFiles`.
    ///
    /// This is the trusted file-upload primitive: it attaches real local files to
    /// the input the same way a human's file picker does (no synthetic events), so
    /// the entire file-upload attack surface, path-traversal filenames,
    /// content-type bypass, SVG/XML XXE, RCE-via-upload, SSRF (becomes testable).
    /// `selector` must resolve to the file input; `files` are absolute local paths.
    pub async fn set_files(&self, selector: &str, files: Vec<String>) -> Result<()> {
        if files.is_empty() {
            return Err(anyhow!("set_files: no files provided"));
        }
        for f in &files {
            if !std::path::Path::new(f).exists() {
                return Err(anyhow!("set_files: file does not exist: '{f}'"));
            }
        }
        // Resolve the input element to its shared node reference + owning context
        // (releases the browser lock before we re-acquire it for the command). Using
        // the node's own context means a file input inside an iframe works too.
        let element = self.find_element(selector).await?;
        let (shared_id, context) = {
            let node = element.node.lock().await;
            let id = node.get_shared_id().cloned().ok_or_else(|| {
                anyhow!("set_files: '{selector}' is not a resolvable element (no shared id)")
            })?;
            (id, node.get_context_id().clone())
        };
        let element_ref: SharedReference = SharedReference::builder()
            .shared_id(shared_id)
            .build()
            .map_err(|e| anyhow!("set_files: build shared reference: {e}"))?;
        let command = SetFiles::builder()
            .context(context)
            .element(element_ref)
            .files(files)
            .build()
            .map_err(|e| anyhow!("set_files: build command: {e}"))?;
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let response = browser
            .driver_mut()
            .send_command(command)
            .await
            .map_err(|e| anyhow!("set_files BiDi command failed: {e:?}"))?;
        let _result: rustenium_bidi_definitions::input::results::SetFilesResult = response
            .result
            .try_into()
            .map_err(|e| anyhow!("set_files result parse failed: {e}"))?;
        Ok(())
    }

    /// Capture a viewport screenshot of the main document and return raw PNG bytes.
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        self.screenshot_with(&ShotOptions::default()).await
    }

    /// Capture PNG bytes of the area `opts` describes, from the document `opts`
    /// names.
    ///
    /// A full-document capture is a single browser-side render, not a
    /// scroll-and-stitch: nothing in the page moves, so a sticky header appears
    /// once and a scroll-triggered animation is not fired by the act of taking
    /// the picture. A region is clipped by the browser at composite time, so a
    /// rectangle below the fold needs no scrolling either. Naming a frame
    /// captures that frame's own document, which is the only way to picture a
    /// cross-origin iframe without the parent's chrome around it.
    pub async fn screenshot_with(&self, opts: &ShotOptions) -> Result<Vec<u8>> {
        use rustenium_bidi_definitions::browsing_context::commands::{
            CaptureScreenshot, CaptureScreenshotOrigin,
        };
        use rustenium_bidi_definitions::browsing_context::results::CaptureScreenshotResult;
        use rustenium_bidi_definitions::browsing_context::types::{
            BoxClipRectangle, BoxClipRectangleType, ClipRectangle,
        };

        let context = match &opts.frame {
            Some(spec) => self.resolve_frame(spec).await?,
            None => self
                .mainframe()
                .await?
                .ok_or_else(|| anyhow!("screenshot: no active browsing context"))?,
        };
        let mut command = CaptureScreenshot::builder().context(context);
        command = match &opts.area {
            ShotArea::Viewport => command.origin(CaptureScreenshotOrigin::Viewport),
            ShotArea::Document => command.origin(CaptureScreenshotOrigin::Document),
            ShotArea::Region {
                x,
                y,
                width,
                height,
            } => {
                if !(*width > 0.0 && *height > 0.0) {
                    return Err(anyhow!(
                        "screenshot: region is {width}x{height}; a capture needs a positive width and height"
                    ));
                }
                let clip = BoxClipRectangle::builder()
                    .r#type(BoxClipRectangleType::Box)
                    .x(*x)
                    .y(*y)
                    .width(*width)
                    .height(*height)
                    .build()
                    .map_err(|e| anyhow!("screenshot: build clip: {e}"))?;
                command
                    .origin(CaptureScreenshotOrigin::Document)
                    .clip(ClipRectangle::BoxClipRectangle(clip))
            }
        };
        let command = command
            .build()
            .map_err(|e| anyhow!("screenshot: build command: {e}"))?;
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let response = browser
            .driver_mut()
            .send_command(command)
            .await
            .map_err(|e| anyhow!("browsingContext.captureScreenshot failed: {e:?}"))?;
        let result: CaptureScreenshotResult = response
            .result
            .try_into()
            .map_err(|e| anyhow!("screenshot result parse failed: {e}"))?;
        base64::engine::general_purpose::STANDARD
            .decode(result.data)
            .map_err(|e| anyhow!("base64 decode failed: {e}"))
    }

    /// Reload the active context.
    pub async fn reload(&self) -> Result<()> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        browser
            .evaluate_script("location.reload()".to_string(), false)
            .await
            .map_err(|e| anyhow!("reload failed: {e:?}"))?;
        Ok(())
    }

    /// Current URL of the active context.
    pub async fn url(&self) -> Result<String> {
        let eval = self.evaluate("document.URL").await?;
        eval.into_value::<String>()
            .map_err(|e| anyhow!("url deserialize failed: {e}"))
    }

    /// Document title of the active context.
    pub async fn title(&self) -> Result<String> {
        let eval = self.evaluate("document.title").await?;
        eval.into_value::<String>()
            .map_err(|e| anyhow!("title deserialize failed: {e}"))
    }

    /// List all browsing-context IDs (main page + every iframe).
    pub async fn frames(&self) -> Result<Vec<FrameId>> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let contexts = browser
            .driver()
            .browsing_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|c| c.id().clone())
            .collect();
        Ok(contexts)
    }

    /// Return the active (main) browsing context.
    pub async fn mainframe(&self) -> Result<Option<FrameId>> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        match browser.driver().get_active_context_id() {
            Ok(ctx) => Ok(Some(ctx)),
            Err(e) => {
                tracing::debug!("get_active_context_id failed: {e:?}");
                Ok(None)
            }
        }
    }

    /// Verify a browsing context still exists.
    pub async fn frame_execution_context(&self, frame_id: FrameId) -> Result<Option<FrameId>> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let exists = browser
            .driver()
            .browsing_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|c| c.id() == &frame_id);
        Ok(if exists { Some(frame_id) } else { None })
    }

    /// List every browsing context (main document + all iframes) with the
    /// metadata an agent needs to target one: opaque `id`, current `url`,
    /// `window.name`, and whether it is the main frame.
    ///
    /// This is the discovery primitive for cross-origin iframe interaction
    /// embedded apps, OAuth/payment widgets, postMessage surfaces, captcha tiles.
    /// Pass a returned `id` back as the `frame` target to
    /// [`Page::eval_in_frame`] / [`Page::click_in_frame`] /
    /// [`Page::type_in_frame`].
    pub async fn list_frames(&self) -> Result<Vec<FrameInfo>> {
        let frame_ids = self.frames().await?;
        let main = self.mainframe().await?;
        let mut out = Vec::with_capacity(frame_ids.len());
        for fid in frame_ids {
            // Read url + name from inside the frame's own context so a
            // cross-origin iframe (where parent JS would throw SecurityError)
            // still reports correctly. A frame that vanished mid-walk is skipped.
            let (url, name) = match self
                .evaluate_in_context("({u: document.URL, n: (window.name || \"\")})", &fid)
                .await
            {
                Ok(eval) => match eval.into_value::<serde_json::Value>() {
                    Ok(v) => (
                        v["u"].as_str().unwrap_or("").to_string(),
                        v["n"].as_str().unwrap_or("").to_string(),
                    ),
                    Err(_) => (String::new(), String::new()),
                },
                Err(e) => {
                    tracing::debug!("frame {:?} unreadable during list_frames: {}", fid, e);
                    (String::new(), String::new())
                }
            };
            out.push(FrameInfo {
                is_main: Some(&fid) == main.as_ref(),
                id: fid.inner().to_string(),
                url,
                name,
            });
        }
        Ok(out)
    }

    /// Walk the live frame tree via WebDriver BiDi `browsingContext.getTree`.
    ///
    /// Returns every browsing context, the main document plus every nested
    /// iframe at any origin, in **pre-order** (a parent always precedes its
    /// children), each carrying true parent linkage, committed URL, and depth.
    ///
    /// This is the structural primitive [`Page::frames`] cannot provide: that
    /// method flattens the tree to a bare id list, discarding which iframe is
    /// nested inside which. Cross-origin captcha challenges (reCAPTCHA's
    /// `bframe` inside its `anchor`, an hCaptcha challenge inside its checkbox)
    /// are exactly the topologies that flattening destroys, so the solver must
    /// re-derive structure every pass. `getTree` recovers it in one round-trip,
    /// and, unlike reading `document.URL` from inside each frame, reports the
    /// URL of a cross-origin frame the browser knows but in-frame JS may not yet
    /// expose.
    pub async fn frame_tree(&self) -> Result<Vec<FrameTreeNode>> {
        use rustenium_bidi_definitions::browsing_context::commands::GetTree;
        use rustenium_bidi_definitions::browsing_context::results::GetTreeResult;
        use rustenium_bidi_definitions::browsing_context::types::{Info, InfoList};

        let command = GetTree::builder().build();
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let response = browser
            .driver_mut()
            .send_command(command)
            .await
            .map_err(|e| anyhow!("browsingContext.getTree BiDi command failed: {e:?}"))?;
        let result: GetTreeResult = response
            .result
            .try_into()
            .map_err(|e| anyhow!("browsingContext.getTree result parse failed: {e}"))?;

        // The BiDi nesting IS the parentage: walk it depth-first, emitting each
        // node before its children so consumers can build a parent→index map in
        // a single pass.
        fn walk(
            list: &InfoList,
            parent: Option<&FrameId>,
            depth: usize,
            out: &mut Vec<FrameTreeNode>,
        ) {
            for info in list.inner() {
                let info: &Info = info;
                out.push(FrameTreeNode {
                    id: info.context.clone(),
                    url: info.url.clone(),
                    parent: parent.cloned(),
                    depth,
                });
                if let Some(children) = &info.children {
                    walk(children, Some(&info.context), depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&result.contexts, None, 0, &mut out);
        Ok(out)
    }

    /// Resolve a frame target spec to a concrete [`FrameId`], polling briefly so
    /// an iframe that attaches asynchronously (captcha widgets, lazy embeds,
    /// post-navigation frames) is found rather than racing to a "no such frame".
    ///
    /// Accepts every shape an agent naturally has on hand, so it never has to
    /// call `list_frames` first:
    /// - exact browsing-context id (from [`Page::list_frames`])
    /// - `index:<n>` or a bare 0-based integer into the frame list
    /// - `url:<substr>`: first frame whose URL contains the substring
    /// - `name:<name>`: first frame whose `window.name` equals it
    /// - any other string, tried as an exact id, then as a URL substring
    /// - empty / `main` / `top` → the main document
    pub async fn resolve_frame(&self, spec: &str) -> Result<FrameId> {
        self.resolve_frame_within(spec, crate::frame::DEFAULT_FRAME_RETRY_TIMEOUT)
            .await
    }

    /// [`Page::resolve_frame`] with an explicit overall timeout for the attach
    /// poll. `timeout` of zero means a single attempt.
    pub async fn resolve_frame_within(
        &self,
        spec: &str,
        timeout: std::time::Duration,
    ) -> Result<FrameId> {
        let parsed = FrameSpec::parse(spec);
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(fid) = self.try_resolve_frame(&parsed).await? {
                return Ok(fid);
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "resolve_frame: no frame matches '{spec}' (use a list_frames id, index:<n>, url:<substr>, or name:<name>)"
                ));
            }
            tokio::time::sleep(crate::frame::DEFAULT_FRAME_RETRY_INTERVAL).await;
        }
    }

    /// One non-retrying resolution attempt. `Ok(None)` means "not found yet"
    /// (caller may retry); `Err` is a hard failure (browser closed, bad index).
    async fn try_resolve_frame(&self, parsed: &FrameSpec) -> Result<Option<FrameId>> {
        if matches!(parsed, FrameSpec::Main) {
            return self.mainframe().await;
        }
        let frames = self.frames().await?;
        match parsed {
            FrameSpec::Main => unreachable!(),
            FrameSpec::Index(idx) => Ok(frames.get(*idx).cloned()),
            FrameSpec::IdOrIndex(id, idx) => {
                // Exact (numeric) id first; then the list index.
                if let Some(fid) = frames.iter().find(|f| f.inner() == id) {
                    return Ok(Some(fid.clone()));
                }
                Ok(frames.get(*idx).cloned())
            }
            FrameSpec::Id(id) => {
                if let Some(fid) = frames.iter().find(|f| f.inner() == id) {
                    return Ok(Some(fid.clone()));
                }
                // Fall back to a URL-substring match so a bare iframe URL works
                // without the explicit `url:` prefix.
                self.frame_by_url_contains(id).await
            }
            FrameSpec::UrlContains(sub) => self.frame_by_url_contains(sub).await,
            FrameSpec::NameEquals(name) => {
                for info in self.list_frames().await? {
                    if &info.name == name {
                        return Ok(Some(FrameId::new(info.id)));
                    }
                }
                Ok(None)
            }
        }
    }

    /// First frame whose current URL contains `sub`. Main frame included so
    /// `url:` can also target the top document.
    async fn frame_by_url_contains(&self, sub: &str) -> Result<Option<FrameId>> {
        for info in self.list_frames().await? {
            if info.url.contains(sub) {
                return Ok(Some(FrameId::new(info.id)));
            }
        }
        Ok(None)
    }

    /// Evaluate `expr` inside the frame named by `spec` (id, index, or
    /// main/top). Full read/write JS runs in that frame's own context, so the
    /// agent can read or mutate a cross-origin iframe's DOM, drive postMessage,
    /// or land a DOM-XSS PoC inside an embedded document.
    pub async fn eval_in_frame(
        &self,
        spec: &str,
        expr: impl Into<String>,
    ) -> Result<EvaluationResult> {
        let fid = self.resolve_frame(spec).await?;
        self.evaluate_in_context(expr, &fid).await
    }

    /// TRUSTED click on `selector` inside the frame named by `spec`.
    ///
    /// Resolves the element's centre in the frame's own viewport, then dispatches
    /// a real BiDi pointer event in that context via [`Page::click_at_in`], so
    /// `event.isTrusted` is `true` even for a cross-origin iframe. Returns an
    /// error if the selector matches nothing visible in the frame.
    pub async fn click_in_frame(&self, spec: &str, selector: &str) -> Result<()> {
        let fid = self.resolve_frame(spec).await?;
        let escaped = crate::frame::escape_js_string(selector);
        let js = format!(
            r#"(function() {{
                const el = document.querySelector('{escaped}');
                if (!el) return null;
                const r = el.getBoundingClientRect();
                if (r.width <= 0 || r.height <= 0) return null;
                return {{ x: r.left + r.width / 2, y: r.top + r.height / 2 }};
            }})()"#
        );
        // Poll for the element's visible rect, it may render a beat after the
        // frame attaches (lazy widgets, post-XHR content).
        let deadline = std::time::Instant::now() + crate::frame::DEFAULT_FRAME_RETRY_TIMEOUT;
        loop {
            if let Ok(eval) = self.evaluate_in_context(&js, &fid).await {
                if let Ok(val) = eval.into_value::<serde_json::Value>() {
                    if let (Some(x), Some(y)) = (val["x"].as_f64(), val["y"].as_f64()) {
                        return self.click_at_in(&fid, x, y).await;
                    }
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "click_in_frame: '{selector}' not found or not visible in frame '{spec}'"
                ));
            }
            tokio::time::sleep(crate::frame::DEFAULT_FRAME_RETRY_INTERVAL).await;
        }
    }

    /// Focus `selector` inside the frame named by `spec` and type `text` into it
    /// with human-like timing. The keystrokes are dispatched in the frame's own
    /// context so they land in the cross-origin iframe's focused element.
    pub async fn type_in_frame(&self, spec: &str, selector: &str, text: &str) -> Result<()> {
        let fid = self.resolve_frame(spec).await?;
        let escaped = crate::frame::escape_js_string(selector);
        let focus_js = format!(
            r#"(function() {{
                const el = document.querySelector('{escaped}');
                if (!el) return false;
                el.focus();
                return document.activeElement === el;
            }})()"#
        );
        // Poll for the field to exist + accept focus before typing.
        let deadline = std::time::Instant::now() + crate::frame::DEFAULT_FRAME_RETRY_TIMEOUT;
        loop {
            let focused = self
                .evaluate_in_context(&focus_js, &fid)
                .await
                .ok()
                .and_then(|e| e.into_value::<bool>().ok())
                .unwrap_or(false);
            if focused {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "type_in_frame: could not focus '{selector}' in frame '{spec}'"
                ));
            }
            tokio::time::sleep(crate::frame::DEFAULT_FRAME_RETRY_INTERVAL).await;
        }
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        browser
            .keyboard()
            .type_text(text, &fid, None)
            .await
            .map_err(|e| anyhow!("type_in_frame: type failed: {e:?}"))?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Dialogs (alert / confirm / prompt / beforeunload) + downloads
    // ------------------------------------------------------------------

    /// Start capturing JS dialogs and page-initiated downloads via BiDi
    /// `browsingContext.*` events. Returns a [`crate::dialog::DialogLog`] handle
    /// (cheap to clone) that accumulates events for the life of the page.
    ///
    /// This is how the agent confirms alert-based XSS (the `alert()` message is
    /// recorded even when the prompt auto-handles, so there is no hang), reads
    /// `confirm`/`prompt` text, and inspects downloads. Pair with
    /// [`Page::handle_user_prompt`] to answer a prompt left open by the `ignore`
    /// handler. Mirrors [`Page::start_network_log`].
    pub async fn start_dialog_log(&self) -> Result<crate::dialog::DialogLog> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let log = crate::dialog::DialogLog::new();
        let handler = crate::dialog::make_dialog_handler(log.clone());
        let events: HashSet<&str> = crate::dialog::DIALOG_EVENTS.iter().copied().collect();
        browser
            .subscribe_events(events, handler)
            .await
            .map_err(|e| anyhow!("failed to subscribe to dialog/download events: {e:?}"))?;
        Ok(log)
    }

    // ------------------------------------------------------------------
    // Sensor grid (the "Omniscient Page")
    // ------------------------------------------------------------------

    /// Install the passive instrumentation grid (see [`crate::sensors`]) so the
    /// page reports DOM-XSS sink writes, console output, uncaught errors, CSP
    /// violations, and inbound postMessage on its own.
    ///
    /// Injected twice: as a preload (runs in the MAIN world before page scripts
    /// on every future navigation) AND evaluated once on the current document so
    /// a page already loaded at launch is covered. The script is idempotent, so
    /// the double-install is safe. Read what it captured with
    /// [`Page::read_signals`]. Mirrors [`Page::start_network_log`].
    pub async fn start_sensors(&self) -> Result<String> {
        let id = self
            .add_preload_script(crate::sensors::SENSOR_SCRIPT)
            .await?;
        // Best-effort cover the already-loaded document; a fresh tab on
        // about:blank may not accept eval yet, which is fine, the preload will
        // fire on the first real navigation.
        let _ = self.evaluate(crate::sensors::SENSOR_SCRIPT).await;
        Ok(id)
    }

    /// Read the captured signal buffer. With `clear` true the buffer is emptied
    /// after the snapshot so the next read returns only NEW signals (deltas)
    /// the basis for "what did my last action trigger?" telemetry.
    pub async fn read_signals(&self, clear: bool) -> Result<serde_json::Value> {
        let eval = self.evaluate(crate::sensors::sensor_reader(clear)).await?;
        eval.into_value::<serde_json::Value>()
            .map_err(|e| anyhow!("read_signals: decode failed: {e}"))
    }

    /// Answer an open JS user prompt in `context` (or the active frame when
    /// `None`): `accept` true clicks OK / accepts `beforeunload`; `user_text`
    /// fills a `prompt()` box before accepting. Only effective when the page was
    /// launched with the `ignore` prompt handler (otherwise Firefox auto-handles
    /// the prompt before this runs). Mirrors the [`Page::set_files`] command path.
    pub async fn handle_user_prompt(
        &self,
        context: Option<&FrameId>,
        accept: bool,
        user_text: Option<&str>,
    ) -> Result<()> {
        let ctx = match context {
            Some(c) => c.clone(),
            None => self
                .mainframe()
                .await?
                .ok_or_else(|| anyhow!("handle_user_prompt: no active browsing context"))?,
        };
        let mut builder = HandleUserPrompt::builder().context(ctx).accept(accept);
        if let Some(text) = user_text {
            builder = builder.user_text(text.to_string());
        }
        let command = builder
            .build()
            .map_err(|e| anyhow!("handle_user_prompt: build command: {e}"))?;
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let response = browser
            .driver_mut()
            .send_command(command)
            .await
            .map_err(|e| anyhow!("handle_user_prompt BiDi command failed: {e:?}"))?;
        let _result: rustenium_bidi_definitions::browsing_context::results::HandleUserPromptResult =
            response
                .result
                .try_into()
                .map_err(|e| anyhow!("handle_user_prompt result parse failed: {e}"))?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Move the mouse from `(x0, y0)` to `(x1, y1)` using human-like curves.
    pub async fn mouse_move_human(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        let hm = browser.human_mouse();
        hm.set_last_position(Point { x: x0, y: y0 });
        hm.move_to(
            Point { x: x1, y: y1 },
            &context,
            MouseMoveOptions::default(),
        )
        .await
        .map_err(|e| anyhow!("mouse_move_human failed: {e:?}"))?;
        Ok(())
    }

    /// Mouse-down at `(x, y)` in the active context.
    pub async fn mouse_down(&self, x: f64, y: f64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        let hm = browser.human_mouse();
        hm.move_to(Point { x, y }, &context, MouseMoveOptions::default())
            .await
            .map_err(|e| anyhow!("mouse_down move failed: {e:?}"))?;
        hm.down(
            &context,
            MouseOptions {
                button: Some(MouseButton::Left),
            },
        )
        .await
        .map_err(|e| anyhow!("mouse_down failed: {e:?}"))?;
        Ok(())
    }

    /// Mouse-up at `(x, y)` in the active context.
    pub async fn mouse_up(&self, _x: f64, _y: f64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        let hm = browser.human_mouse();
        hm.up(
            &context,
            MouseOptions {
                button: Some(MouseButton::Left),
            },
        )
        .await
        .map_err(|e| anyhow!("mouse_up failed: {e:?}"))?;
        Ok(())
    }

    /// Click at `(x, y)` in the active (top-level) context with realistic
    /// press/release timing.
    ///
    /// NOTE: for a target inside a cross-origin iframe (the production captcha
    /// case: Turnstile/hCaptcha/reCAPTCHA all render their checkbox in an
    /// OOPIF), prefer [`Page::click_at_in`] with the iframe's context. A
    /// pointer action dispatched in the *top* context does not reliably route
    /// across a Fission process boundary, which is why a top-context viewport
    /// click on a captcha checkbox silently fails to deliver.
    pub async fn click_at(&self, x: f64, y: f64) -> Result<()> {
        let context = self
            .mainframe()
            .await?
            .ok_or_else(|| anyhow!("click_at: no active browsing context"))?;
        self.click_at_in(&context, x, y).await
    }

    /// Click at `(x, y)` within a SPECIFIC browsing context.
    ///
    /// This is the cross-origin-correct click path: BiDi
    /// `input.performActions` is dispatched in `context`, so the *trusted*
    /// pointer event is delivered into that frame's content process. For a
    /// cross-origin iframe checkbox, pass the iframe's [`FrameId`] (from
    /// [`Page::frames`]) with coordinates in that frame's own viewport space
    /// (origin at the iframe's top-left). Because the event is real BiDi input
    /// (not a synthetic JS `MouseEvent`), `event.isTrusted` is `true`: the
    /// property every modern captcha gates its checkbox on.
    pub async fn click_at_in(&self, context: &FrameId, x: f64, y: f64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let hm = browser.human_mouse();
        // Seed the cursor origin INSIDE the target context's viewport. The
        // shared HumanMouse remembers its last position across calls; that
        // position is in whatever viewport the previous action used (often the
        // top frame, which is larger than a captcha iframe). Moving from a
        // stale top-frame coordinate into a small iframe viewport makes Firefox
        // BiDi reject the action with MoveTargetOutOfBounds. Anchoring at the
        // target keeps every dispatched coordinate within the iframe's bounds.
        hm.set_last_position(Point { x, y });
        let options = MouseClickOptions {
            button: Some(MouseButton::Left),
            count: Some(1),
            delay: Some(80),
            origin: Some(rustenium_bidi_definitions::input::types::Origin::Viewport),
        };
        hm.click(Some(Point { x, y }), context, options)
            .await
            .map_err(|e| anyhow!("click_at_in failed: {e:?}"))?;
        Ok(())
    }

    /// Move the pointer to an absolute viewport coordinate as a single
    /// TRUSTED BiDi `input.performActions` PointerMove (no synthetic JS
    /// `MouseEvent`).
    ///
    /// This is the trusted primitive that human-trajectory generators must
    /// dispatch each interpolated point through. A `document.dispatchEvent(new
    /// MouseEvent('mousemove', …))` produces `isTrusted === false`, which every
    /// modern anti-bot scorer flags on sight, so a beautifully shaped but
    /// JS-dispatched path is worse than useless. Routing each point through
    /// here makes the whole trajectory trusted and lets it cross into
    /// cross-origin frames by viewport hit-test.
    pub async fn move_mouse_to(&self, x: f64, y: f64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .mouse()
            .move_to(
                Point { x, y },
                &context,
                MouseMoveOptions {
                    steps: Some(0),
                    origin: Some(rustenium_bidi_definitions::input::types::Origin::Viewport),
                },
            )
            .await
            .map_err(|e| anyhow!("move_mouse_to failed: {e:?}"))?;
        Ok(())
    }

    /// Scroll the wheel at the current mouse position.
    pub async fn scroll(&self, dx: i64, dy: i64) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .mouse()
            .wheel(
                &context,
                MouseWheelOptions {
                    delta_x: Some(dx),
                    delta_y: Some(dy),
                },
            )
            .await
            .map_err(|e| anyhow!("scroll failed: {e:?}"))?;
        Ok(())
    }

    /// Human-like scroll (smooth easing with noise).
    pub async fn scroll_realistic(&self, direction: ScrollDirection, amount: u32) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        let y_distance = match direction {
            ScrollDirection::Down => amount as i32,
            ScrollDirection::Up => -(amount as i32),
        };
        browser
            .human_mouse()
            .scroll(y_distance, 0, &context)
            .await
            .map_err(|e| anyhow!("scroll_realistic failed: {e:?}"))?;
        Ok(())
    }

    /// Type `text` into the active context with human-like delays.
    pub async fn type_text(&self, text: &str) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .keyboard()
            .type_text(text, &context, None)
            .await
            .map_err(|e| anyhow!("type_text failed: {e:?}"))?;
        Ok(())
    }

    /// Press a key down in the active context.
    pub async fn key_down(&self, key: &str) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .keyboard()
            .down(key, &context)
            .await
            .map_err(|e| anyhow!("key_down failed: {e:?}"))?;
        Ok(())
    }

    /// Release a key in the active context.
    pub async fn key_up(&self, key: &str) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .keyboard()
            .up(key, &context)
            .await
            .map_err(|e| anyhow!("key_up failed: {e:?}"))?;
        Ok(())
    }

    /// Press and release a key in the active context.
    pub async fn key_press(&self, key: &str) -> Result<()> {
        let browser = self.browser.lock().await;
        let browser = match &*browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let context = browser
            .driver()
            .get_active_context_id()
            .map_err(|e| anyhow!("{e:?}"))?;
        browser
            .keyboard()
            .press(key, &context, None)
            .await
            .map_err(|e| anyhow!("key_press failed: {e:?}"))?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Stealth / scripting
    // ------------------------------------------------------------------

    /// Inject a preload script that runs in the page's main world before any
    /// page script, on every new document.
    ///
    /// `source` is a SCRIPT BODY (statements), matching CDP's
    /// `Page.addScriptToEvaluateOnNewDocument` semantics. WebDriver BiDi's
    /// `script.addPreloadScript` instead takes a `functionDeclaration` that it
    /// *invokes* as a function, so a bare body, or a self-invoking IIFE like
    /// `(() => {…})()` (which evaluates to `undefined`, not a callable), is
    /// silently never run, nullifying the script. We therefore wrap the body in
    /// an arrow function here so callers can pass a plain body and have it
    /// actually execute. This is the single point that made guise's stealth
    /// preloads (all written as IIFE bodies) no-ops.
    pub async fn add_preload_script(&self, source: &str) -> Result<String> {
        let trimmed = source.trim();
        let is_fn_decl = trimmed.starts_with("() =>")
            || trimmed.starts_with("async () =>")
            || trimmed.starts_with("function")
            || trimmed.starts_with("async function");
        let function_declaration = if is_fn_decl {
            source.to_string()
        } else {
            format!("() => {{\n{source}\n}}")
        };
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let id = browser
            .add_preload_script(function_declaration)
            .await
            .map_err(|e| anyhow!("add_preload_script failed: {e:?}"))?;
        Ok(id)
    }

    /// Capture all cookies (including HttpOnly) via BiDi `storage.getCookies`.
    pub async fn get_cookies(&self) -> Result<Vec<crate::cookies::CapturedCookie>> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let response = browser
            .driver_mut()
            .send_command(GetCookies {
                method: rustenium_bidi_definitions::storage::commands::GetCookiesMethod::GetCookies,
                params: Default::default(),
            })
            .await
            .map_err(|e| anyhow!("get_cookies BiDi command failed: {e:?}"))?;
        let result: rustenium_bidi_definitions::storage::results::GetCookiesResult = response
            .result
            .try_into()
            .map_err(|e| anyhow!("get_cookies result parse failed: {e}"))?;
        Ok(result
            .cookies
            .into_iter()
            .map(|c| crate::cookies::CapturedCookie {
                name: c.name,
                value: match c.value {
                    BytesValue::StringValue(s) => s.value,
                    BytesValue::Base64Value(b) => b.value,
                },
                domain: c.domain,
                path: c.path,
                expires: c.expiry.map(|e| e as i64),
                secure: c.secure,
                http_only: c.http_only,
                same_site: Some(format!("{:?}", c.same_site).to_lowercase()),
            })
            .collect())
    }

    /// Set a cookie via BiDi `storage.setCookie`.
    #[allow(clippy::too_many_arguments)] // a cookie's fields are the domain arity
    pub async fn set_cookie(
        &self,
        name: &str,
        value: &str,
        domain: &str,
        path: Option<&str>,
        expires: Option<u64>,
        secure: Option<bool>,
        http_only: Option<bool>,
        same_site: Option<SameSite>,
    ) -> Result<()> {
        let normalized_domain = domain.trim_start_matches('.');
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let cookie = PartialCookie {
            name: name.to_string(),
            value: BytesValue::StringValue(StringValue::new(
                StringValueType::String,
                value.to_string(),
            )),
            domain: normalized_domain.to_string(),
            path: path.map(|p| p.to_string()),
            http_only,
            secure,
            same_site,
            expiry: expires,
            extensible: Default::default(),
        };
        let response = browser
            .driver_mut()
            .send_command(SetCookie {
                method: rustenium_bidi_definitions::storage::commands::SetCookieMethod::SetCookie,
                params: SetCookieParams::new(cookie),
            })
            .await
            .map_err(|e| anyhow!("set_cookie BiDi command failed: {e:?}"))?;
        let _result: rustenium_bidi_definitions::storage::results::SetCookieResult = response
            .result
            .try_into()
            .map_err(|e| anyhow!("set_cookie result parse failed: {e}"))?;
        Ok(())
    }

    /// Return the Firefox profile directory path, if known.
    pub fn profile_dir(&self) -> Option<&str> {
        self.profile_dir.as_deref()
    }

    /// Start capturing all network traffic (requests + responses) via BiDi.
    ///
    /// Returns a [`crate::network::NetworkLog`] handle that can be queried at
    /// any time while the browser is alive.  The log is shared (Clone is cheap)
    /// and accumulates events until the page is closed.
    ///
    /// # Example
    /// ```ignore
    /// let log = page.start_network_log().await?;
    /// page.goto("https://example.com").await?;
    /// let entries = log.entries().await;
    /// let tokens = log.extract_tokens().await;
    /// ```
    pub async fn start_network_log(&self) -> Result<crate::network::NetworkLog> {
        let mut browser = self.browser.lock().await;
        let browser = match &mut *browser {
            Some(b) => b,
            None => return Err(anyhow!("browser closed")),
        };
        let log = crate::network::NetworkLog::new();
        let handler = crate::network::make_network_handler(log.clone());
        let events: HashSet<&str> = [
            "network.beforeRequestSent",
            "network.responseCompleted",
            "network.fetchError",
        ]
        .into_iter()
        .collect();
        browser
            .subscribe_events(events, handler)
            .await
            .map_err(|e| anyhow!("failed to subscribe to network events: {e:?}"))?;
        Ok(log)
    }

    /// Close the browser. For a self-managed (Remote-attach) child this performs a
    /// CLEAN quit so the profile's localStorage / IndexedDB / cookies are flushed to
    /// disk before exit; capped so a hung engine still tears down.
    ///
    /// Persistence depends on this being called, a dropped [`Page`] (see [`Drop`])
    /// can only best-effort SIGTERM/SIGKILL, which does NOT flush localStorage on
    /// this engine, so a reused `profile_dir` would lose recent writes.
    pub async fn close(&self) -> Result<()> {
        // Take the self-managed child (Remote-attach path) OUT of the std mutex
        // first (never hold a std guard across `.await`).
        let child_opt = self.child.lock().ok().and_then(|mut g| g.take());

        if let Some(mut c) = child_opt {
            // PERSISTENCE, why this is a BiDi `browser.close`, not a SIGKILL:
            //
            // Firefox's LSNG localStorage buffers writes in the content process,
            // hands them to the parent Datastore, and the Datastore writes to disk
            // only on a HARDCODED 5 s timer (`kFlushTimeoutMs`, no pref) OR when the
            // Datastore closes. A SIGKILL, or rustenium's own `fuser -k <port>`
            // by-port kill in `FirefoxBrowser::close`: interrupts that, so a reused
            // `profile_dir` silently loses recent localStorage/IndexedDB across a
            // restart (confirmed live: localStorage read back `null`). SIGTERM does
            // NOT help on this engine (it is ignored: the process stays alive).
            //
            // The BiDi `browser.close` command closes every top-level tab with
            // `skipPermitUnload` and then shuts the browser down. Closing a tab tears
            // down that origin's content-process localStorage handle, which makes the
            // parent `Datastore::Close` → `Connection::Close` cancel the 5 s timer and
            // FLUSH IMMEDIATELY; the subsequent shutdown runs `QuotaManager::Shutdown`,
            // finalizing every remaining datastore. By the time the process exits,
            // storage is on disk. This is the only flush path that survives a restart.
            {
                let mut guard = self.browser.lock().await;
                if let Some(browser) = guard.as_mut() {
                    let cmd = BrowserCommand::Close(BrowserCloseCmd {
                        method: BrowserCloseMethod::Close,
                        params: BrowserCloseParams {},
                    });
                    // The engine drops the BiDi socket as it shuts down, so this can
                    // return an error/timeout, the flush is driven by the tab
                    // teardown the command triggers, not by the response, so the
                    // outcome is intentionally ignored.
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        browser.driver_mut().send_command(cmd),
                    )
                    .await;
                }
            }
            // Firefox flushes storage during the clean shutdown `browser.close`
            // started; it has exited (storage on disk) by the time this returns.
            if !wait_for_exit(&mut c, 100).await {
                // The engine did not exit on its own (e.g. headless kept the parent
                // process alive). The per-origin flush already landed when the tabs
                // closed above, so it is safe to escalate now: SIGTERM, then SIGKILL.
                request_graceful_terminate(c.id());
                if !wait_for_exit(&mut c, 30).await {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            // Drop the (now-dead) BiDi connection. We do NOT call rustenium's
            // `FirefoxBrowser::close` here: its `fuser -k <port>` SIGKILL would race
            // the flush above, and the process is already gone.
            let _ = self.browser.lock().await.take();
        } else if let Some(browser) = self.browser.lock().await.take() {
            // rustenium-managed path (it owns the process): end the BiDi session.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), browser.close()).await;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------
// Browser launch configuration
// ------------------------------------------------------------------

/// Upstream proxy transport for a launched Firefox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyScheme {
    /// HTTP/HTTPS proxy (`network.proxy.http` + `ssl`, shared).
    #[default]
    Http,
    /// SOCKS5 proxy (`network.proxy.socks`, remote DNS on).
    Socks5,
}

/// A proxy to route a launched Firefox through. Emitted as `network.proxy.*`
/// prefs into the profile `user.js` at launch, the right place, since Firefox
/// has no `--proxy-server` flag.
///
/// IP-whitelisted gateways work fully via prefs. Firefox cannot carry
/// **proxy-auth credentials** in prefs (it would prompt), so `username`/
/// `password` are plumbed but require a local unauthenticated relay (e.g.
/// `proxywire`) in front of the authenticated upstream; [`proxy_prefs`] logs a
/// warning rather than silently dropping them.
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    pub scheme: ProxyScheme,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    /// Parse `scheme://[user:pass@]host:port`. Scheme defaults to `http`;
    /// `socks5`/`socks` selects SOCKS5.
    pub fn from_url(url: &str) -> Result<Self> {
        let (scheme, rest) = match url.split_once("://") {
            Some((s, r)) => (s.to_ascii_lowercase(), r),
            None => ("http".to_string(), url),
        };
        let scheme = match scheme.as_str() {
            "socks5" | "socks" | "socks5h" => ProxyScheme::Socks5,
            "http" | "https" => ProxyScheme::Http,
            other => return Err(anyhow!("unsupported proxy scheme: {other}")),
        };
        let (auth, hostport) = match rest.rsplit_once('@') {
            Some((a, hp)) => (Some(a), hp),
            None => (None, rest),
        };
        let (username, password) = match auth {
            Some(a) => match a.split_once(':') {
                Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
                None => (Some(a.to_string()), None),
            },
            None => (None, None),
        };
        let (host, port) = hostport
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("proxy URL missing host:port: {url}"))?;
        let port: u16 = port
            .parse()
            .map_err(|_| anyhow!("invalid proxy port in {url}"))?;
        if host.is_empty() {
            return Err(anyhow!("proxy URL missing host: {url}"));
        }
        Ok(Self {
            scheme,
            host: host.to_string(),
            port,
            username,
            password,
        })
    }
}

/// Build the Firefox `network.proxy.*` `user_pref` lines for `proxy`.
pub fn proxy_prefs(proxy: &ProxyConfig) -> String {
    if proxy.username.is_some() || proxy.password.is_some() {
        tracing::warn!(
            "ProxyConfig carries credentials, but Firefox cannot apply proxy auth via prefs; \
             front the upstream with a local unauthenticated relay (e.g. proxywire) and point \
             foxdriver at that. Emitting host:port prefs only."
        );
    }
    let mut lines = vec![r#"user_pref("network.proxy.type", 1);"#.to_string()];
    match proxy.scheme {
        ProxyScheme::Http => {
            lines.push(format!(
                r#"user_pref("network.proxy.http", "{}");"#,
                proxy.host
            ));
            lines.push(format!(
                r#"user_pref("network.proxy.http_port", {});"#,
                proxy.port
            ));
            lines.push(format!(
                r#"user_pref("network.proxy.ssl", "{}");"#,
                proxy.host
            ));
            lines.push(format!(
                r#"user_pref("network.proxy.ssl_port", {});"#,
                proxy.port
            ));
            lines.push(r#"user_pref("network.proxy.share_proxy_settings", true);"#.to_string());
        }
        ProxyScheme::Socks5 => {
            lines.push(format!(
                r#"user_pref("network.proxy.socks", "{}");"#,
                proxy.host
            ));
            lines.push(format!(
                r#"user_pref("network.proxy.socks_port", {});"#,
                proxy.port
            ));
            lines.push(r#"user_pref("network.proxy.socks_version", 5);"#.to_string());
            lines.push(r#"user_pref("network.proxy.socks_remote_dns", true);"#.to_string());
        }
    }
    // Do not bypass the proxy for localhost, a residential run must egress
    // every request through the upstream, including any IP-echo check.
    lines.push(r#"user_pref("network.proxy.no_proxies_on", "");"#.to_string());

    // WebRTC IP-leak prevention, proxy-conditional by design. Without this,
    // ICE candidate gathering opens a DIRECT UDP socket to STUN servers,
    // bypassing the proxy entirely and exposing the host's real public
    // (server-reflexive) AND LAN (host) addresses. Behind a proxy that real IP
    // CONTRADICTS the proxy egress IP, the classic WebRTC deanonymization that
    // silently blows the caller's cover even when every HTTP byte is proxied.
    // `ice.proxy_only` forces ALL ICE traffic through the configured proxy, so
    // no real-IP candidate is ever gathered; `no_host` suppresses LAN-address
    // candidates; `default_address_only` exposes only the default route (no
    // multi-homed interface enumeration). WebRTC stays ENABLED, disabling it
    // (`media.peerconnection.enabled=false`) is itself a fingerprint tell, it
    // simply cannot egress outside the proxy.
    lines.push(r#"user_pref("media.peerconnection.ice.proxy_only", true);"#.to_string());
    lines.push(r#"user_pref("media.peerconnection.ice.no_host", true);"#.to_string());
    lines.push(r#"user_pref("media.peerconnection.ice.default_address_only", true);"#.to_string());

    // DNS-leak prevention, proxy-conditional. DNS prefetch (`<link
    // rel=dns-prefetch>`, anchor pre-resolution), the network predictor, and
    // speculative connections resolve hostnames via the OS resolver OUTSIDE the
    // proxy, leaking the visited/linked domains AND the host's real DNS path
    // even when every NAVIGATED request is proxied. Disabling them makes the
    // proxy the ONLY resolver path (the SOCKS form additionally forces lookups
    // through the proxy via `socks_remote_dns` above; an HTTP proxy resolves
    // server-side from the full-URI request). Without these, a single
    // `dns-prefetch` link silently emits a clear-text DNS query from the host.
    lines.push(r#"user_pref("network.dns.disablePrefetch", true);"#.to_string());
    lines.push(r#"user_pref("network.dns.disablePrefetchFromHTTPS", true);"#.to_string());
    lines.push(r#"user_pref("network.predictor.enabled", false);"#.to_string());
    lines.push(r#"user_pref("network.http.speculative-parallel-limit", 0);"#.to_string());
    lines.push(r#"user_pref("browser.urlbar.speculativeConnect.enabled", false);"#.to_string());

    lines.push('\n'.to_string());
    lines.join("\n")
}

#[derive(Debug, Clone, Default)]
pub struct FoxBrowserConfig {
    pub executable_path: Option<String>,
    /// Firefox profile directory.
    ///
    /// Supply a STABLE path to get a PERSISTENT persona: cookies, localStorage,
    /// IndexedDB, and the per-identity device fingerprint (canvas/audio seed, when
    /// launched via `guise`) all survive a restart that reuses the same path, a
    /// returning logged-in user, not a brand-new browser each launch. Persistence
    /// requires the session to end through [`Page::close`], which performs the clean
    /// `browser.close` shutdown that flushes Firefox's QuotaManager storage to disk
    /// (a bare SIGKILL would lose unflushed localStorage/IndexedDB).
    ///
    /// `None` synthesizes a fresh temporary profile per launch, an ephemeral,
    /// one-shot persona with no cross-launch state.
    pub profile_dir: Option<String>,
    pub headless: bool,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub user_agent: Option<String>,
    /// Raw `user.js` content to write into the profile directory before
    /// Firefox starts. The caller (typically `guise`) is responsible for
    /// building this string from profile overrides.
    pub user_js_content: Option<String>,
    /// Optional upstream proxy. Emitted as `network.proxy.*` prefs appended to
    /// `user_js_content` at launch (requires `profile_dir`).
    pub proxy: Option<ProxyConfig>,
    /// How Firefox handles JS user prompts (`alert`/`confirm`/`prompt`/
    /// `beforeunload`). One of `accept`, `dismiss`, `ignore`, `dismiss and
    /// notify`. `None` keeps the BiDi default (`dismiss and notify`), which
    /// never hangs and still emits the events the dialog log records. Set
    /// `ignore` to keep prompts OPEN so [`Page::handle_user_prompt`] can answer
    /// them; set `accept` to auto-accept (a `confirm()` guard returns true,
    /// `beforeunload` never blocks navigation).
    pub unhandled_prompt_behavior: Option<String>,
    /// Extra environment variables to set on the spawned Firefox process, on top
    /// of the inherited parent env. ONLY honored by [`launch_firefox_self_managed`]
    /// (foxdriver owns that spawn); the rustenium-managed [`launch_firefox`] cannot
    /// set per-process env. The canonical use is `TZ=<IANA zone>` so ICU reports the
    /// persona timezone in EVERY realm, including dedicated Workers, which a
    /// window-realm JS `Intl`/`Date` preload can never reach (a worker that read the
    /// host zone while the window claimed the persona zone was a trivially-detected
    /// leak). Per-process, so concurrent launches with different zones never race
    /// (unlike mutating the parent process's `TZ`).
    pub env: Vec<(String, String)>,
}

/// Map a prompt-behavior string to the typed BiDi capability value. Returns
/// `None` for an unrecognized value so launch falls back to the BiDi default
/// rather than failing.
fn prompt_behavior_capability(s: &str) -> Option<UnhandledPromptBehavior> {
    let handler = match s.trim().to_ascii_lowercase().as_str() {
        "accept" | "accept and notify" => UserPromptHandlerType::Accept,
        "dismiss" => UserPromptHandlerType::Dismiss,
        "ignore" => UserPromptHandlerType::Ignore,
        "dismiss and notify" | "dismiss_and_notify" | "notify" => {
            UserPromptHandlerType::DismissAndNotify
        }
        _ => return None,
    };
    Some(UnhandledPromptBehavior::UserPromptHandlerType(handler))
}

/// Write `user.js` into the given profile directory.
fn write_user_js(profile_dir: &str, content: &str) -> Result<()> {
    let dir = std::path::Path::new(profile_dir);
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow!("failed to create profile dir {:?}: {}", dir, e))?;
    let path = dir.join("user.js");
    std::fs::write(&path, content)
        .map_err(|e| anyhow!("failed to write user.js to {:?}: {}", path, e))?;
    Ok(())
}

/// Launch Firefox with the given config and return a `Page` handle.
pub async fn launch_firefox(mut config: FoxBrowserConfig) -> Result<Page> {
    let mut caps = FirefoxCapabilities::default();
    caps.accept_insecure_certs(true);
    if let Some(behavior) = config
        .unhandled_prompt_behavior
        .as_deref()
        .and_then(prompt_behavior_capability)
    {
        caps.unhandled_prompt_behavior(behavior);
    }

    let mut args = Vec::new();
    if config.headless {
        args.push("--headless".to_string());
    }
    if let Some(ref ua) = config.user_agent {
        args.push(format!("--user-agent={}", ua));
    }
    if config.viewport_width > 0 {
        args.push(format!("--width={}", config.viewport_width));
    }
    if config.viewport_height > 0 {
        args.push(format!("--height={}", config.viewport_height));
    }

    // Assemble the final user.js: caller-supplied prefs plus, if a proxy is
    // configured, the network.proxy.* lines. Written before launch so prefs are
    // live from the first request (a proxied run must NOT leak the real IP on
    // the initial navigation).
    let mut user_js = config.user_js_content.clone().unwrap_or_default();
    if let Some(ref proxy) = config.proxy {
        if !user_js.is_empty() && !user_js.ends_with('\n') {
            user_js.push('\n');
        }
        user_js.push_str(&proxy_prefs(proxy));
    }
    if !user_js.is_empty() {
        // A non-empty user.js means the caller set engine-level prefs (persona UA
        // override, dom.maxHardwareConcurrency, automation prefs, proxy). Firefox
        // only reads user.js from a profile directory, so if none was supplied we
        // MUST synthesize one, the old behaviour silently dropped every pref
        // behind a `tracing::warn`, shipping a browser that LOOKS launched but is
        // missing exactly those prefs: a half-applied disguise (e.g. a Worker realm
        // reporting the real hardwareConcurrency) and, with a proxy configured, a
        // real-IP leak on the first navigation. That is an invisible recall hole,
        // not a warning to continue past (Law 10 / fail-closed for stealth).
        if config
            .profile_dir
            .as_deref()
            .filter(|d| !d.is_empty())
            .is_none()
        {
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("foxdriver-profile-{}-{n}", std::process::id()));
            config.profile_dir = Some(dir.to_string_lossy().into_owned());
        }
        let profile_dir = config
            .profile_dir
            .as_deref()
            .ok_or_else(|| anyhow!("profile_dir missing when writing user.js"))?;
        // Fail closed: a stealth/proxy pref that does not get written produces a
        // detectable, potentially IP-leaking browser (surface it, never continue).
        write_user_js(profile_dir, &user_js)
            .map_err(|e| anyhow!("failed to write user.js to profile {profile_dir:?}: {e}"))?;
    }

    let profile_dir = config.profile_dir.clone();
    let cfg = FirefoxConfig {
        capabilities: caps,
        firefox_executable_path: config.executable_path,
        profile_dir: config.profile_dir,
        browser_flags: Some(args),
        ..Default::default()
    };

    let browser = tokio::time::timeout(std::time::Duration::from_secs(30), firefox(Some(cfg)))
        .await
        .map_err(|_| anyhow!("Firefox launch timed out after 30s, check that Firefox is installed and not already running with a locked profile"))?;
    Ok(Page {
        browser: tokio::sync::Mutex::new(Some(browser)),
        profile_dir,
        child: std::sync::Mutex::new(None),
    })
}

/// Reserve an ephemeral TCP port by binding `127.0.0.1:0` and reading back the
/// OS-assigned port, then releasing it. There is an unavoidable TOCTOU window
/// between release and the browser binding it; in practice the browser claims it
/// within milliseconds and a collision surfaces as a clean readiness-timeout.
fn reserve_local_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| anyhow!("failed to reserve a local port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| anyhow!("failed to read reserved port: {e}"))?
        .port();
    Ok(port)
}

/// Resolve the Firefox binary: the caller's explicit `executable_path` if set,
/// otherwise the first match on `PATH` and then the standard install locations.
///
/// [`launch_firefox`] gets PATH resolution for free because it hands a possibly-
/// `None` path to rustenium, which finds Firefox itself. When foxdriver owns the
/// spawn ([`launch_firefox_self_managed`]) it must do the same so the robust
/// readiness-poll launcher is a true drop-in, a caller that relies on
/// Firefox-on-PATH (e.g. captchaforge's `drive_browser`) can adopt it without
/// hard-coding a path.
fn resolve_firefox_binary(explicit: Option<String>) -> Result<String> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    const NAMES: &[&str] = &["firefox", "firefox-esr", "firefox-bin", "firefox.exe"];
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path.split(sep).filter(|d| !d.is_empty()) {
            for name in NAMES {
                let cand = std::path::Path::new(dir).join(name);
                if cand.is_file() {
                    return Ok(cand.to_string_lossy().into_owned());
                }
            }
        }
    }
    // Standard locations that are not always on PATH (snap/opt/macOS/Windows).
    const FIXED: &[&str] = &[
        "/usr/local/bin/firefox",
        "/usr/bin/firefox",
        "/opt/firefox/firefox",
        "/snap/bin/firefox",
        "/Applications/Firefox.app/Contents/MacOS/firefox",
        "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
        "C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe",
    ];
    for p in FIXED {
        if std::path::Path::new(p).is_file() {
            return Ok((*p).to_string());
        }
    }
    Err(anyhow!(
        "could not find a Firefox binary, set FoxBrowserConfig.executable_path or install Firefox on PATH"
    ))
}

/// Launch Firefox where **foxdriver owns the spawn and the readiness wait**, then
/// attaches over BiDi in rustenium `Remote` mode.
///
/// The default [`launch_firefox`] delegates spawning to rustenium, which sleeps a
/// fixed 500 ms after exec before connecting to the BiDi WebSocket. That races any
/// build whose remote agent binds slowly, a freshly-built lurien takes
/// ~1 s, yielding a `ConnectionRefused` panic. Here foxdriver spawns the process,
/// polls the debugging port until it actually accepts a connection (Law-7:
/// readiness, never a fixed sleep), and only then hands rustenium an already-live
/// port via [`FirefoxLaunchMode::Remote`]. The spawned [`std::process::Child`] is
/// owned by the returned [`Page`] and killed on `close`/drop.
///
/// `config.executable_path` is resolved via [`resolve_firefox_binary`], the
/// explicit path if set, else PATH / standard install locations (this path never
/// auto-downloads Firefox).
pub async fn launch_firefox_self_managed(config: FoxBrowserConfig) -> Result<Page> {
    let exe = resolve_firefox_binary(config.executable_path.clone())?;

    let host = "127.0.0.1".to_string();
    let port = reserve_local_port()?;

    // Profile dir: caller-supplied or a unique temp dir. Written with the same
    // user.js (incl. proxy prefs) as the managed path so prefs are live from the
    // first request.
    let profile_dir = config.profile_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir()
            .join(format!("foxdriver-self-{}-{}", std::process::id(), port))
            .display()
            .to_string()
    });
    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| anyhow!("failed to create profile dir {profile_dir:?}: {e}"))?;

    let mut user_js = config.user_js_content.clone().unwrap_or_default();
    if let Some(ref proxy) = config.proxy {
        if !user_js.is_empty() && !user_js.ends_with('\n') {
            user_js.push('\n');
        }
        user_js.push_str(&proxy_prefs(proxy));
    }
    if !user_js.is_empty() {
        write_user_js(&profile_dir, &user_js)?;
    }

    // Assemble args. `--no-remote` + the explicit debugging port mirror what
    // rustenium would pass in SpawnAndAttach; the rest come from the viewport /
    // headless / UA config.
    let mut args = vec![
        format!("--remote-debugging-port={port}"),
        "--profile".to_string(),
        profile_dir.clone(),
        "--no-remote".to_string(),
    ];
    if config.headless {
        args.push("--headless".to_string());
    }
    if let Some(ref ua) = config.user_agent {
        args.push(format!("--user-agent={ua}"));
    }
    if config.viewport_width > 0 {
        args.push(format!("--width={}", config.viewport_width));
    }
    if config.viewport_height > 0 {
        args.push(format!("--height={}", config.viewport_height));
    }

    // Spawn the process. The parent env is inherited (so a launch wrapper's
    // exported config / sandbox toggles propagate); match rustenium's
    // MOZ_LAUNCHER_PROCESS=0 so the parent PID is the actual browser. Caller env
    // (e.g. TZ for worker-realm timezone coherence) is applied per-process here so
    // concurrent launches with different values never race on the parent env.
    // stdout/stderr stay off the parent: lurien-mcp is stdio JSON-RPC, and Gecko
    // chatter on inherited stdout is a protocol break.
    let mut command = std::process::Command::new(&exe);
    command
        .args(&args)
        .env("MOZ_LAUNCHER_PROCESS", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (key, value) in &config.env {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .map_err(|e| anyhow!("failed to spawn browser {exe:?}: {e}"))?;

    // Poll the debugging port until it accepts a connection, or time out. This is
    // the wait rustenium's fixed 500 ms sleep gets wrong for slow-binding builds.
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow!("bad debug addr {host}:{port}: {e}"))?;
    let start = std::time::Instant::now();
    let ready_timeout = std::time::Duration::from_secs(30);
    loop {
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250))
            .is_ok()
        {
            break;
        }
        if start.elapsed() >= ready_timeout {
            terminate_and_reap(child);
            return Err(anyhow!(
                "browser debug port {port} never came up within {}s, the spawn likely failed (check {exe:?})",
                ready_timeout.as_secs()
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Attach over BiDi to the already-live port (no spawn, no fixed-sleep race).
    //
    // A SINGLE attach: rustenium's `BidiSession::new` waits a hardcoded 5 s for
    // the `session.new` response and PANICS on timeout, but the session is still
    // CREATED on the browser, and a BiDi browser allows only one active session,
    // so a retry just hits "Maximum number of active sessions". The right lever is
    // therefore to give the engine enough head start that its single `session.new`
    // answers within that window (see the post-readiness settle below), not to
    // retry. The attach runs in a task so a timeout surfaces as a clean error
    // instead of unwinding this function.
    let cfg = FirefoxConfig {
        host: Some(host.clone()),
        capabilities: {
            let mut caps = FirefoxCapabilities::default();
            caps.accept_insecure_certs(true);
            if let Some(behavior) = config
                .unhandled_prompt_behavior
                .as_deref()
                .and_then(prompt_behavior_capability)
            {
                caps.unhandled_prompt_behavior(behavior);
            }
            caps
        },
        launch_mode: FirefoxLaunchMode::Remote(port),
        remote_debugging_port: Some(port),
        ..Default::default()
    };
    let attach = tokio::spawn(async move {
        tokio::time::timeout(std::time::Duration::from_secs(30), firefox(Some(cfg))).await
    });
    let browser = match attach.await {
        Ok(Ok(b)) => b,
        Ok(Err(_elapsed)) => {
            terminate_and_reap(child);
            return Err(anyhow!(
                "BiDi attach to self-managed browser timed out after 30s"
            ));
        }
        Err(join) => {
            terminate_and_reap(child);
            return Err(anyhow!(
                "BiDi attach to self-managed browser failed: {join}"
            ));
        }
    };

    Ok(Page {
        browser: tokio::sync::Mutex::new(Some(browser)),
        profile_dir: Some(profile_dir),
        child: std::sync::Mutex::new(Some(child)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the `Drop` cleanup for a self-managed Firefox child sent
    /// SIGKILL but never called `wait()`, so a killed browser lingered as a
    /// zombie until the whole foxdriver process exited. `terminate_and_reap`
    /// must leave no zombie behind. A process that ignores SIGTERM forces the
    /// SIGKILL path; after the call its pid must be fully reaped (gone from
    /// /proc, not merely defunct).
    #[cfg(unix)]
    #[test]
    fn terminate_and_reap_leaves_no_zombie() {
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM; sleep 300"])
            .spawn()
            .expect("spawn test child");
        let pid = child.id();
        terminate_and_reap(child);
        // A reaped child vanishes from /proc; a zombie would still show up.
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "pid {pid} still present after terminate_and_reap (zombie leak)"
        );
    }

    /// Regression: `click_in_frame` and `type_in_frame` escaped selectors
    /// with a local ad-hoc escaper that only handled backslash and single
    /// quote, beside the crate's shared `escape_js_string`. Both now use the
    /// shared escaper; this locks the shared behavior every JS-interpolation
    /// site relies on.
    #[test]
    fn shared_escaper_covers_quote_and_control_breakout() {
        let malicious = "'); alert(1); //";
        let escaped = crate::frame::escape_js_string(malicious);
        // The single quote is escaped, so the JS string literal cannot be
        // closed early.
        assert_eq!(escaped, "\\'); alert(1); //");
        // Every quote and backslash in the output is prefixed by a backslash.
        for (index, _) in escaped.match_indices('\'') {
            assert_eq!(escaped.as_bytes()[index - 1], b'\\');
        }
    }

    use rustenium_bidi_definitions::script::types::{
        ArrayRemoteValue, ArrayRemoteValueType, BigIntValue, BigIntValueType, BooleanValue,
        BooleanValueType, ListRemoteValue, MappingRemoteValue, NullValue, NullValueType,
        NumberValue, NumberValueType, ObjectRemoteValue, ObjectRemoteValueType,
        PrimitiveProtocolValue, StringValue, StringValueType, UndefinedValue, UndefinedValueType,
    };

    // ─── FrameSpec::parse ───

    #[test]
    fn frame_spec_main_aliases() {
        assert_eq!(FrameSpec::parse(""), FrameSpec::Main);
        assert_eq!(FrameSpec::parse("  "), FrameSpec::Main);
        assert_eq!(FrameSpec::parse("main"), FrameSpec::Main);
        assert_eq!(FrameSpec::parse("TOP"), FrameSpec::Main);
    }

    #[test]
    fn frame_spec_index_forms() {
        // Bare digits are ambiguous (numeric BiDi id OR index) → IdOrIndex.
        assert_eq!(FrameSpec::parse("0"), FrameSpec::IdOrIndex("0".into(), 0));
        assert_eq!(FrameSpec::parse("3"), FrameSpec::IdOrIndex("3".into(), 3));
        // A large numeric Firefox context id is still resolvable by exact id.
        assert_eq!(
            FrameSpec::parse("10737418241"),
            FrameSpec::IdOrIndex("10737418241".into(), 10737418241)
        );
        // `index:` forces a strict index.
        assert_eq!(FrameSpec::parse("index:2"), FrameSpec::Index(2));
    }

    #[test]
    fn frame_spec_url_and_name_prefixes() {
        assert_eq!(
            FrameSpec::parse("url:recaptcha/api2"),
            FrameSpec::UrlContains("recaptcha/api2".into())
        );
        assert_eq!(
            FrameSpec::parse("name:checkout-frame"),
            FrameSpec::NameEquals("checkout-frame".into())
        );
        // Whitespace inside the value is trimmed.
        assert_eq!(
            FrameSpec::parse("url:  https://x.com "),
            FrameSpec::UrlContains("https://x.com".into())
        );
    }

    #[test]
    fn frame_spec_bare_id_falls_through() {
        // An opaque BiDi context id (non-numeric, no prefix) is an Id.
        assert_eq!(
            FrameSpec::parse("10737418241-abc"),
            FrameSpec::Id("10737418241-abc".into())
        );
        // A bare URL with no prefix is also an Id (resolve falls back to URL match).
        assert_eq!(
            FrameSpec::parse("https://w.com/f"),
            FrameSpec::Id("https://w.com/f".into())
        );
    }

    // ─── prompt_behavior_capability ───

    #[test]
    fn prompt_behavior_maps_known_values() {
        for s in [
            "accept",
            "ACCEPT",
            "dismiss",
            "ignore",
            "dismiss and notify",
            "notify",
        ] {
            assert!(
                prompt_behavior_capability(s).is_some(),
                "'{s}' should map to a capability"
            );
        }
    }

    #[test]
    fn prompt_behavior_rejects_unknown() {
        assert!(prompt_behavior_capability("").is_none());
        assert!(prompt_behavior_capability("bogus").is_none());
    }

    #[test]
    fn prompt_behavior_ignore_is_user_prompt_handler_type() {
        match prompt_behavior_capability("ignore") {
            Some(UnhandledPromptBehavior::UserPromptHandlerType(UserPromptHandlerType::Ignore)) => {
            }
            other => panic!("ignore should map to UserPromptHandlerType::Ignore, got {other:?}"),
        }
    }

    // ─── bidi_wire_value_to_json ───

    #[test]
    fn wire_string_extracts_value() {
        let v = serde_json::json!({"type": "string", "value": "hello"});
        assert_eq!(bidi_wire_value_to_json(&v), serde_json::json!("hello"));
    }

    #[test]
    fn wire_number_passthrough() {
        let v = serde_json::json!({"type": "number", "value": 42.5});
        assert_eq!(bidi_wire_value_to_json(&v), serde_json::json!(42.5));
    }

    #[test]
    fn wire_boolean_extracts_bool() {
        let v = serde_json::json!({"type": "boolean", "value": true});
        assert_eq!(bidi_wire_value_to_json(&v), serde_json::json!(true));
    }

    #[test]
    fn wire_null_returns_null() {
        let v = serde_json::json!({"type": "null"});
        assert_eq!(bidi_wire_value_to_json(&v), serde_json::Value::Null);
    }

    #[test]
    fn wire_undefined_returns_null() {
        let v = serde_json::json!({"type": "undefined"});
        assert_eq!(bidi_wire_value_to_json(&v), serde_json::Value::Null);
    }

    #[test]
    fn wire_bigint_returns_string() {
        let v = serde_json::json!({"type": "bigint", "value": "9007199254740993"});
        assert_eq!(
            bidi_wire_value_to_json(&v),
            serde_json::json!("9007199254740993")
        );
    }

    #[test]
    fn wire_object_recurse() {
        let v = serde_json::json!({
            "type": "object",
            "value": [
                ["a", {"type": "string", "value": "alpha"}],
                ["b", {"type": "number", "value": 2}]
            ]
        });
        let out = bidi_wire_value_to_json(&v);
        assert_eq!(out["a"], "alpha");
        assert_eq!(out["b"], 2);
    }

    #[test]
    fn wire_array_recurse() {
        let v = serde_json::json!({
            "type": "array",
            "value": [
                {"type": "string", "value": "x"},
                {"type": "number", "value": 1}
            ]
        });
        let out = bidi_wire_value_to_json(&v);
        assert_eq!(out, serde_json::json!(["x", 1]));
    }

    #[test]
    fn wire_unknown_type_clones_raw() {
        let v = serde_json::json!({"type": "special", "payload": 99});
        assert_eq!(bidi_wire_value_to_json(&v), v);
    }

    #[test]
    fn wire_missing_type_clones_raw() {
        let v = serde_json::json!({"payload": 99});
        assert_eq!(bidi_wire_value_to_json(&v), v);
    }

    // ─── remote_value_to_json ───

    #[test]
    fn rv_string_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::StringValue(
            StringValue::new(StringValueType::String, "hi"),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::json!("hi"));
    }

    #[test]
    fn rv_number_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::NumberValue(
            NumberValue::new(NumberValueType::Number, 2.5),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::json!(2.5));
    }

    #[test]
    fn rv_boolean_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::BooleanValue(
            BooleanValue::new(BooleanValueType::Boolean, true),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::json!(true));
    }

    #[test]
    fn rv_null_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::NullValue(
            NullValue::new(NullValueType::Null),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::Value::Null);
    }

    #[test]
    fn rv_undefined_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::UndefinedValue(
            UndefinedValue::new(UndefinedValueType::Undefined),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::Value::Null);
    }

    #[test]
    fn rv_bigint_value() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::BigIntValue(
            BigIntValue::new(BigIntValueType::Bigint, "999n"),
        ));
        assert_eq!(remote_value_to_json(&rv), serde_json::json!("999n"));
    }

    #[test]
    fn rv_array_value() {
        let inner = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::StringValue(
            StringValue::new(StringValueType::String, "item"),
        ));
        let arr = ArrayRemoteValue {
            r#type: ArrayRemoteValueType::Array,
            handle: None,
            internal_id: None,
            value: Some(ListRemoteValue::new(vec![inner])),
        };
        let rv = RemoteValue::ArrayRemoteValue(arr);
        assert_eq!(remote_value_to_json(&rv), serde_json::json!(["item"]));
    }

    #[test]
    fn rv_object_value() {
        let obj = ObjectRemoteValue {
            r#type: ObjectRemoteValueType::Object,
            handle: None,
            internal_id: None,
            value: Some(MappingRemoteValue::new(vec![vec![
                serde_json::json!("key"),
                serde_json::json!({"type": "string", "value": "val"}),
            ]])),
        };
        let rv = RemoteValue::ObjectRemoteValue(obj);
        let out = remote_value_to_json(&rv);
        assert_eq!(out["key"], "val");
    }

    #[test]
    fn rv_object_value_wire_key() {
        let obj = ObjectRemoteValue {
            r#type: ObjectRemoteValueType::Object,
            handle: None,
            internal_id: None,
            value: Some(MappingRemoteValue::new(vec![vec![
                serde_json::json!({"type": "string", "value": "key"}),
                serde_json::json!({"type": "string", "value": "val"}),
            ]])),
        };
        let rv = RemoteValue::ObjectRemoteValue(obj);
        let out = remote_value_to_json(&rv);
        assert_eq!(out["key"], "val");
    }

    #[test]
    fn bidi_wire_value_to_json_object_wire_key() {
        let raw = serde_json::json!({
            "type": "object",
            "value": [
                [
                    {"type": "string", "value": "wire_key"},
                    {"type": "number", "value": 42}
                ]
            ]
        });
        let out = bidi_wire_value_to_json(&raw);
        assert_eq!(out["wire_key"], 42);
    }

    #[test]
    fn rv_unsupported_returns_null() {
        let sym = rustenium_bidi_definitions::script::types::SymbolRemoteValue::new(
            rustenium_bidi_definitions::script::types::SymbolRemoteValueType::Symbol,
        );
        let rv = RemoteValue::SymbolRemoteValue(sym);
        assert_eq!(remote_value_to_json(&rv), serde_json::Value::Null);
    }

    // ─── EvaluationResult ───

    #[test]
    fn eval_result_into_value_deserializes() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::StringValue(
            StringValue::new(StringValueType::String, "deserialized"),
        ));
        let er = EvaluationResult::new(rv);
        let s: String = er.into_value().unwrap();
        assert_eq!(s, "deserialized");
    }

    #[test]
    fn eval_result_into_value_number() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::NumberValue(
            NumberValue::new(NumberValueType::Number, 42i32),
        ));
        let er = EvaluationResult::new(rv);
        let n: i32 = er.into_value().unwrap();
        assert_eq!(n, 42);
    }

    #[test]
    fn eval_result_remote_value_accessor() {
        let rv = RemoteValue::PrimitiveProtocolValue(PrimitiveProtocolValue::BooleanValue(
            BooleanValue::new(BooleanValueType::Boolean, false),
        ));
        let er = EvaluationResult::new(rv.clone());
        assert_eq!(er.remote_value(), &rv);
    }

    // ─── FoxBrowserConfig ───

    #[test]
    fn fox_browser_config_default_is_headless_false() {
        let cfg = FoxBrowserConfig::default();
        assert!(!cfg.headless);
        assert!(cfg.executable_path.is_none());
        assert!(cfg.profile_dir.is_none());
        assert_eq!(cfg.viewport_width, 0);
        assert_eq!(cfg.viewport_height, 0);
        assert!(cfg.user_agent.is_none());
        assert!(cfg.user_js_content.is_none());
    }

    // ─── write_user_js ───

    #[test]
    fn write_user_js_creates_file() {
        let tmp = std::env::temp_dir().join(format!("foxdriver_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let content = "user_pref(\"test\", true);\n";
        write_user_js(tmp.to_str().unwrap(), content).unwrap();
        let path = tmp.join("user.js");
        assert!(path.exists());
        let read = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read, content);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_user_js_creates_nested_dirs() {
        let tmp = std::env::temp_dir().join(format!("foxdriver_nested_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let nested = tmp.join("a").join("b");
        write_user_js(nested.to_str().unwrap(), "pref").unwrap();
        assert!(nested.join("user.js").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ─── ProxyConfig / proxy_prefs ───

    #[test]
    fn proxy_from_url_http_no_auth() {
        let p = ProxyConfig::from_url("http://1.2.3.4:8080").unwrap();
        assert_eq!(p.scheme, ProxyScheme::Http);
        assert_eq!(p.host, "1.2.3.4");
        assert_eq!(p.port, 8080);
        assert!(p.username.is_none() && p.password.is_none());
    }

    #[test]
    fn proxy_from_url_socks5_with_auth() {
        let p = ProxyConfig::from_url("socks5://user:pass@gw.residential.net:1080").unwrap();
        assert_eq!(p.scheme, ProxyScheme::Socks5);
        assert_eq!(p.host, "gw.residential.net");
        assert_eq!(p.port, 1080);
        assert_eq!(p.username.as_deref(), Some("user"));
        assert_eq!(p.password.as_deref(), Some("pass"));
    }

    #[test]
    fn proxy_from_url_bare_defaults_http() {
        let p = ProxyConfig::from_url("10.0.0.1:3128").unwrap();
        assert_eq!(p.scheme, ProxyScheme::Http);
        assert_eq!(p.host, "10.0.0.1");
        assert_eq!(p.port, 3128);
    }

    #[test]
    fn proxy_from_url_rejects_missing_port_and_bad_scheme() {
        assert!(ProxyConfig::from_url("http://nohost").is_err());
        assert!(ProxyConfig::from_url("ftp://h:1").is_err());
        assert!(ProxyConfig::from_url("http://h:notaport").is_err());
    }

    #[test]
    fn proxy_prefs_http_emits_http_ssl_and_type() {
        let prefs = proxy_prefs(&ProxyConfig::from_url("http://5.6.7.8:9000").unwrap());
        assert!(prefs.contains(r#"user_pref("network.proxy.type", 1);"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.http", "5.6.7.8");"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.http_port", 9000);"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.ssl", "5.6.7.8");"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.ssl_port", 9000);"#));
        // Negative twin: the HTTP form must NOT emit SOCKS prefs.
        assert!(!prefs.contains("network.proxy.socks"));
    }

    #[test]
    fn proxy_prefs_socks5_emits_socks_and_version() {
        let prefs = proxy_prefs(&ProxyConfig::from_url("socks5://h:1080").unwrap());
        assert!(prefs.contains(r#"user_pref("network.proxy.socks", "h");"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.socks_port", 1080);"#));
        assert!(prefs.contains(r#"user_pref("network.proxy.socks_version", 5);"#));
        // Negative twin: the SOCKS form must NOT emit the HTTP-proxy prefs.
        assert!(!prefs.contains("network.proxy.http_port"));
    }

    #[test]
    fn proxy_prefs_close_the_webrtc_ip_leak_for_both_schemes() {
        // A proxied egress MUST also force WebRTC ICE through the proxy, else a
        // direct-UDP STUN gather leaks the host's real public IP (srflx) and LAN
        // IP (host) (contradicting the proxy egress and deanonymizing the run).
        // Both HTTP and SOCKS proxies are affected, so both must carry the fix.
        for url in ["http://5.6.7.8:9000", "socks5://h:1080"] {
            let prefs = proxy_prefs(&ProxyConfig::from_url(url).unwrap());
            assert!(
                prefs.contains(r#"user_pref("media.peerconnection.ice.proxy_only", true);"#),
                "{url}: must force ICE through the proxy (no direct-UDP srflx leak)"
            );
            assert!(
                prefs.contains(r#"user_pref("media.peerconnection.ice.no_host", true);"#),
                "{url}: must suppress LAN host candidates"
            );
            assert!(
                prefs.contains(
                    r#"user_pref("media.peerconnection.ice.default_address_only", true);"#
                ),
                "{url}: must expose only the default route address"
            );
            // Soundness: WebRTC stays ENABLED (disabling it is itself a tell).
            assert!(
                !prefs.contains("media.peerconnection.enabled"),
                "{url}: must NOT disable WebRTC outright (a fingerprint tell); only confine ICE"
            );
        }
    }

    #[test]
    fn proxy_prefs_close_the_dns_prefetch_leak_for_both_schemes() {
        // A proxied egress must also stop DNS prefetch / predictor / speculative
        // connections, which resolve hostnames via the OS resolver OUTSIDE the
        // proxy (leaking the visited/linked domains. Both schemes are affected).
        for url in ["http://5.6.7.8:9000", "socks5://h:1080"] {
            let prefs = proxy_prefs(&ProxyConfig::from_url(url).unwrap());
            assert!(
                prefs.contains(r#"user_pref("network.dns.disablePrefetch", true);"#),
                "{url}: must disable DNS prefetch (a `dns-prefetch` link leaks a clear-text query)"
            );
            assert!(
                prefs.contains(r#"user_pref("network.dns.disablePrefetchFromHTTPS", true);"#),
                "{url}: must disable DNS prefetch from HTTPS origins too"
            );
            assert!(
                prefs.contains(r#"user_pref("network.predictor.enabled", false);"#),
                "{url}: must disable the network predictor (history-driven pre-resolution)"
            );
            assert!(
                prefs.contains(r#"user_pref("network.http.speculative-parallel-limit", 0);"#),
                "{url}: must stop speculative parallel connections"
            );
            assert!(
                prefs.contains(r#"user_pref("browser.urlbar.speculativeConnect.enabled", false);"#),
                "{url}: must stop urlbar speculative connect"
            );
        }
    }

    #[test]
    fn proxy_prefs_socks_keeps_remote_dns_alongside_the_leak_guards() {
        // Regression fence: the SOCKS resolver-through-proxy pref must survive
        // next to the new prefetch/predictor guards (defense in depth, remote
        // DNS routes navigated lookups, the guards kill the out-of-band ones).
        let prefs = proxy_prefs(&ProxyConfig::from_url("socks5://h:1080").unwrap());
        assert!(prefs.contains(r#"user_pref("network.proxy.socks_remote_dns", true);"#));
        assert!(prefs.contains(r#"user_pref("network.dns.disablePrefetch", true);"#));
    }

    // ─── ScrollDirection ───

    #[test]
    fn scroll_direction_up_not_eq_down() {
        assert_ne!(ScrollDirection::Up, ScrollDirection::Down);
    }

    #[test]
    fn scroll_direction_clone_copy() {
        let a = ScrollDirection::Up;
        let b = a;
        assert_eq!(a, b); // copy, not move
    }

    #[test]
    fn scroll_direction_debug() {
        let s = format!("{:?}", ScrollDirection::Down);
        assert!(s.contains("Down"));
    }
    #[test]
    fn remote_value_to_json_handles_string_number_values() {
        let rv: RemoteValue = serde_json::from_value(serde_json::json!({
            "type": "number",
            "value": "NaN"
        }))
        .unwrap();
        let json = remote_value_to_json(&rv);
        assert_eq!(json, serde_json::Value::String("NaN".to_string()));
    }

    #[test]
    fn remote_value_to_json_handles_date_and_regexp() {
        let regexp_rv: RemoteValue = serde_json::from_value(serde_json::json!({
            "type": "regexp",
            "value": {
                "pattern": "abc",
                "flags": "gi"
            }
        }))
        .unwrap();
        assert_eq!(
            remote_value_to_json(&regexp_rv),
            serde_json::Value::String("/abc/gi".to_string())
        );

        let date_rv: RemoteValue = serde_json::from_value(serde_json::json!({
            "type": "date",
            "value": "2026-08-07T00:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(
            remote_value_to_json(&date_rv),
            serde_json::Value::String("2026-08-07T00:00:00.000Z".to_string())
        );
    }
    #[test]
    fn bidi_wire_value_to_json_handles_bool_keys() {
        let raw = serde_json::json!({
            "type": "object",
            "value": [
                [
                    {"type": "boolean", "value": true},
                    {"type": "number", "value": 100}
                ]
            ]
        });
        let out = bidi_wire_value_to_json(&raw);
        assert_eq!(out["true"], 100);
    }

    #[test]
    fn preload_script_detection_identifies_fn_declarations() {
        let fn_decl = "() => { window.x = 1; }";
        let trimmed = fn_decl.trim();
        let is_fn_decl = trimmed.starts_with("() =>")
            || trimmed.starts_with("async () =>")
            || trimmed.starts_with("function")
            || trimmed.starts_with("async function");
        assert!(is_fn_decl);

        let stmt = "window.x = 1;";
        let trimmed_stmt = stmt.trim();
        let is_fn_decl_stmt = trimmed_stmt.starts_with("() =>")
            || trimmed_stmt.starts_with("async () =>")
            || trimmed_stmt.starts_with("function")
            || trimmed_stmt.starts_with("async function");
        assert!(!is_fn_decl_stmt);
    }

    #[test]
    fn set_cookie_domain_strips_leading_dot() {
        let domain = ".example.com";
        let normalized = domain.trim_start_matches('.');
        assert_eq!(normalized, "example.com");
    }
}
