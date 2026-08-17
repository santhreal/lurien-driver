//! The API. Every face is a transport over [`Session::call`].
//!
//! A session owns at most one engine process plus the passive telemetry that
//! process produces: the network log, the dialog log, and the sensor grid. The
//! page is launched on first use so a face can list verbs, print help, or answer
//! `tools/list` before an engine exists, and still fail with the missing-engine
//! sentence the moment a verb needs pixels.

use crate::error::Error;
use crate::launch::LaunchOptions;
use crate::verb::{self, Args, Output};
use crate::Browser;
use runtime_foxdriver::{DialogLog, NetworkLog};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Passive capture attached at launch. Every handle is cheap to clone and
/// accumulates until the page closes.
#[derive(Clone)]
pub struct Telemetry {
    /// Every request and response the browser made.
    pub network: NetworkLog,
    /// Every JavaScript dialog and download.
    pub dialogs: DialogLog,
    /// Whether the DOM-signal preload script is installed.
    pub sensors: bool,
}

/// One browser session, shared by every face.
pub struct Session {
    opts: LaunchOptions,
    browser: Mutex<Option<Arc<Browser>>>,
    telemetry: Mutex<Option<Telemetry>>,
    /// Routes this session has added, in the order the engine tries them.
    routes: Mutex<Vec<crate::route::Route>>,
    /// Names this session has given the frames it has seen.
    frames: Mutex<crate::frame::Handles>,
    /// Why this session has no position state, when it has none.
    geo_refusal: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self::with_options(LaunchOptions::default())
    }
}

impl Session {
    /// Default persona, headful, engine resolved on first verb.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicit launch options.
    #[must_use]
    pub fn with_options(opts: LaunchOptions) -> Self {
        let mut opts = opts;
        // Resolved here, not at launch, so a verb can read where this session's
        // downloads land before the engine exists.
        if opts.download_dir.is_none() {
            opts.download_dir = Some(crate::download::session_dir());
        }
        // Same reason, and one more: the engine is handed the control channel in
        // its environment at startup, so the port and the token have to exist
        // before the first launch. A persona whose timezone names no region
        // serves no position, which the geolocation verbs report rather than
        // inventing coordinates.
        let mut geo_refusal = None;
        if opts.geo.is_none() {
            match crate::geo::Geolocation::new(
                crate::geo::persona_position(opts.profile),
                opts.geolocation,
            ) {
                Ok(state) => opts.geo = Some(Arc::new(state)),
                Err(e) => geo_refusal = Some(e.to_string()),
            }
        }
        Self {
            opts,
            browser: Mutex::new(None),
            telemetry: Mutex::new(None),
            routes: Mutex::new(Vec::new()),
            frames: Mutex::new(crate::frame::Handles::default()),
            geo_refusal,
        }
    }

    /// The position this session serves, and the channel that moves it.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when no control channel could be reserved,
    /// which leaves the session with no way to reach the engine's own state.
    pub fn geo(&self) -> Result<&Arc<crate::geo::Geolocation>, Error> {
        self.opts
            .geo
            .as_ref()
            .ok_or_else(|| Error::ControlUnavailable {
                detail: self
                    .geo_refusal
                    .clone()
                    .unwrap_or_else(|| "no control channel was reserved".to_string()),
            })
    }

    /// The privileged channel into this session's engine.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when no channel could be reserved.
    pub fn control(&self) -> Result<&crate::control::Control, Error> {
        Ok(self.geo()?.control())
    }

    /// Add a route, tried before every route added before it, and report the
    /// whole table.
    ///
    /// The engine holds one table and tries it in order, so a new route is a
    /// whole-table write. Most recent first means a caller narrows behaviour by
    /// adding a route rather than by having to withdraw one.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine is not reachable or
    /// refuses the route, which leaves the session's table as it was.
    pub async fn add_route(&self, route: crate::route::Route) -> Result<serde_json::Value, Error> {
        let mut guard = self.routes.lock().await;
        let mut next = Vec::with_capacity(guard.len() + 1);
        next.push(route);
        next.extend(guard.iter().cloned());
        self.control()?
            .set_routes(&crate::route::table_json(&next))
            .await?;
        *guard = next;
        Ok(report(&guard, &self.control()?.routes().await?))
    }

    /// Forget every route this session added.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine is not reachable.
    pub async fn clear_routes(&self) -> Result<usize, Error> {
        let mut guard = self.routes.lock().await;
        self.control()?.clear_routes().await?;
        let dropped = guard.len();
        guard.clear();
        Ok(dropped)
    }

    /// The route table in match order, with how many requests each route took.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine is not reachable.
    pub async fn route_report(&self) -> Result<serde_json::Value, Error> {
        let guard = self.routes.lock().await;
        Ok(report(&guard, &self.control()?.routes().await?))
    }

    /// Every frame in the page right now, each with the name this session gave
    /// it. Reading the tree is what mints a handle, so a caller that has run
    /// `frames` can address any of them.
    pub async fn frame_rows(&self) -> Result<Vec<serde_json::Value>, Error> {
        let tree = self.read_frames().await?;
        // The tree does not carry `window.name`, which is one of the specs a
        // caller may pass, so it is read separately and matched by context.
        let browser = self.browser().await?;
        if let Ok(named) = browser.page().list_frames().await {
            let mut guard = self.frames.lock().await;
            for frame in &named {
                if !frame.name.is_empty() {
                    guard.set_name(&frame.id, &frame.name);
                }
            }
        }
        let guard = self.frames.lock().await;
        Ok(tree
            .iter()
            .map(|node| {
                let context = node.id.as_ref();
                serde_json::json!({
                    "handle": guard.handle_for(context),
                    "id": context,
                    "url": node.url,
                    "name": guard.slot_for(context).map_or("", |slot| slot.name.as_str()),
                    "parent": node.parent.as_ref().map(AsRef::<str>::as_ref),
                    "depth": node.depth,
                    "is_main": node.depth == 0,
                })
            })
            .collect())
    }

    /// Turn what a caller wrote into something the engine can resolve.
    ///
    /// A handle is looked up in this session's table; anything else is the
    /// engine's own spec language (id, `index:`, `url:`, `name:`, `main`) and
    /// passes through untouched.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when a handle names a frame that is gone or a frame
    /// this session never saw. Neither is resolved to a different frame: acting
    /// on the wrong document is the failure a handle exists to prevent.
    pub async fn frame_target(&self, verb: &str, spec: &str) -> Result<String, Error> {
        let Some(handle) = crate::frame::parse_handle(spec) else {
            return Ok(spec.to_string());
        };
        // A handle may be older than the last time anything read the tree, so the
        // table is refreshed before it is believed either way.
        self.read_frames().await?;
        let guard = self.frames.lock().await;
        match guard.slot(handle) {
            Some(slot) if slot.live => Ok(slot.context.clone()),
            Some(slot) => Err(crate::frame::gone(verb, slot)),
            None => Err(crate::frame::unknown(verb, handle, guard.slots())),
        }
    }

    /// The live tree, with the table brought up to date from it.
    async fn read_frames(&self) -> Result<Vec<runtime_foxdriver::FrameTreeNode>, Error> {
        let browser = self.browser().await?;
        // The tree, not the driver's context list: a frame the page removed is
        // still in that list, and a handle that resolved off it would address a
        // context the browser has already dropped.
        let tree = browser
            .page()
            .frame_tree()
            .await
            .map_err(|e| Error::Other(format!("reading the frame tree: {e}")))?;
        let live: Vec<(String, String)> = tree
            .iter()
            .map(|node| (node.id.as_ref().to_string(), node.url.clone()))
            .collect();
        self.frames.lock().await.refresh(&live);
        Ok(tree)
    }

    /// Launch options this session will use.
    #[must_use]
    pub fn options(&self) -> &LaunchOptions {
        &self.opts
    }

    /// The page, launching the engine on first use.
    pub async fn browser(&self) -> Result<Arc<Browser>, Error> {
        let mut guard = self.browser.lock().await;
        if let Some(browser) = guard.as_ref() {
            return Ok(Arc::clone(browser));
        }
        let browser = Arc::new(Browser::launch_with_options(self.opts.clone()).await?);
        let telemetry = attach_telemetry(&browser).await;
        *self.telemetry.lock().await = Some(telemetry);
        *guard = Some(Arc::clone(&browser));
        Ok(browser)
    }

    /// Telemetry handles for the live page, launching it if needed. Capture is
    /// started at launch, so a verb that reads it never has to arm it first and
    /// can never report an empty log as "no traffic".
    pub async fn telemetry(&self) -> Result<Telemetry, Error> {
        self.browser().await?;
        self.telemetry
            .lock()
            .await
            .clone()
            .ok_or_else(|| Error::Other("telemetry unavailable for this session".into()))
    }

    /// Replace the live page, closing the old one. Used by `as`, which relaunches
    /// wearing an imported profile.
    pub async fn adopt(&self, browser: Browser) {
        let browser = Arc::new(browser);
        let telemetry = attach_telemetry(&browser).await;
        let previous = self.browser.lock().await.replace(Arc::clone(&browser));
        *self.telemetry.lock().await = Some(telemetry);
        if let Some(old) = previous {
            if let Some(old) = Arc::into_inner(old) {
                let _ = old.close().await;
            }
        }
    }

    /// True once an engine is running.
    pub async fn is_open(&self) -> bool {
        self.browser.lock().await.is_some()
    }

    /// Current URL, or an empty string when nothing is open. Faces use this for
    /// envelope fields; it never launches an engine.
    pub async fn current_url(&self) -> String {
        let live = self.browser.lock().await.clone();
        match live {
            Some(browser) => browser.url().await.unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Run one verb. The only entry point a face may use.
    pub async fn call(&self, name: &str, args: &Args) -> Result<Output, Error> {
        let spec = verb::lookup(name).ok_or_else(|| Error::UnknownVerb {
            name: name.to_string(),
        })?;
        spec.call(self, args).await
    }

    /// Close the engine if this session opened one.
    pub async fn close(&self) -> Result<(), Error> {
        let taken = self.browser.lock().await.take();
        *self.telemetry.lock().await = None;
        match taken.and_then(Arc::into_inner) {
            Some(browser) => browser.close().await,
            None => Ok(()),
        }
    }
}

/// One row per route, in match order, with the driver's detail and the engine's
/// count. The engine reports the table it was given, so the two agree by
/// position; a row the engine does not report is still shown, without a count,
/// rather than dropped.
fn report(routes: &[crate::route::Route], engine: &serde_json::Value) -> serde_json::Value {
    let counted = engine.as_array().map(std::vec::Vec::as_slice).unwrap_or_default();
    let rows: Vec<serde_json::Value> = routes
        .iter()
        .enumerate()
        .map(|(index, route)| {
            let mut row = route.row();
            let hits = counted
                .get(index)
                .filter(|reported| reported.get("pattern") == Some(&serde_json::json!(route.pattern)))
                .and_then(|reported| reported.get("hits").cloned());
            if let (Some(hits), Some(map)) = (hits, row.as_object_mut()) {
                map.insert("hits".to_string(), hits);
            }
            row
        })
        .collect();
    serde_json::json!({ "routes": rows, "count": rows.len() })
}

/// Arm passive capture. A log that fails to start is empty, never absent, so a
/// telemetry verb reports "nothing captured" instead of a driver error.
async fn attach_telemetry(browser: &Browser) -> Telemetry {
    let page = browser.page();
    let network = page.start_network_log().await.unwrap_or_default();
    let dialogs = page.start_dialog_log().await.unwrap_or_default();
    let sensors = sensors_enabled() && page.start_sensors().await.is_ok();
    Telemetry {
        network,
        dialogs,
        sensors,
    }
}

/// DOM-signal capture is on by default. `LURIEN_SENSORS=0` turns it off for a
/// run that must not carry a preload script.
fn sensors_enabled() -> bool {
    match std::env::var("LURIEN_SENSORS") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "1" | "true" | "yes" | "on"
        ),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_verb_is_named() {
        let session = Session::new();
        let err = session
            .call("teleport", &Args::new())
            .await
            .expect_err("unknown verb");
        assert!(err.to_string().contains("teleport"), "{err}");
    }

    #[tokio::test]
    async fn bad_arguments_fail_before_the_engine_is_touched() {
        // No LURIEN_BIN needed: validation runs before any launch, so this is
        // an argument error, never the missing-engine sentence.
        let session = Session::new();
        let err = session
            .call("goto", &Args::new())
            .await
            .expect_err("url is required");
        assert!(err.to_string().contains("url"), "{err}");
        assert!(!session.is_open().await);
    }

    #[tokio::test]
    async fn url_of_a_closed_session_is_empty_and_launches_nothing() {
        let session = Session::new();
        assert_eq!(session.current_url().await, "");
        assert!(!session.is_open().await);
    }

    #[test]
    fn sensors_default_on_and_switch_off() {
        std::env::remove_var("LURIEN_SENSORS");
        assert!(sensors_enabled());
        std::env::set_var("LURIEN_SENSORS", "0");
        assert!(!sensors_enabled());
        std::env::set_var("LURIEN_SENSORS", "on");
        assert!(sensors_enabled());
        std::env::remove_var("LURIEN_SENSORS");
    }
}
