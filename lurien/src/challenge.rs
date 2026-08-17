//! The engine-level solver, configured and read from here.
//!
//! The solve does not happen in this process. It happens in the browser, in the
//! parent process, with one actor per browsing context, because the widget is a
//! cross-origin document that page script cannot read and an outside driver
//! cannot measure. This module hands the engine three things and reads one back:
//!
//! - the catalog, already parsed from `captcha/kinds/` at build time, so the
//!   browser holds no TOML reader and cannot disagree with the driver about what
//!   a page is,
//! - a deck of pointer trajectories, drag profiles and preludes, each sampled
//!   from the same persona corpus as the rest of the session, so a click matches
//!   the identity that made the TLS handshake and two clicks in one session do
//!   not match each other, and the kinds this build is allowed to act on,
//! - an evidence path.
//!
//! It reads back the evidence: what the engine saw, what it did, and whether the
//! vendor wrote a token. A run with no evidence line is reported as no engine
//! solve, never as a pass.

use crate::catalog;
use guise::human::mouse::MouseSampler;
use rand::SeedableRng;
use std::path::{Path, PathBuf};

/// Environment variable the engine reads its challenge configuration from.
pub const CONFIG_ENV: &str = "LURIEN_CHALLENGE";

/// Kinds this build acts on. Everything else is refused by the engine with a
/// typed error rather than reported as a pass. An interactive kind joins this
/// list only with a dated scorecard row against a live vendor page, which
/// `tests/kinds_registry.rs` enforces.
pub const CLAIMED_KINDS: &[&str] = &["none", "score", "checkbox", "pow", "slider", "fail"];

/// Schema version of every row the engine appends to the evidence file, and the
/// only version this build reads.
///
/// The evidence file is the driver's only account of what the browser did, and the
/// two halves ship separately: a driver from one build reads rows from an engine
/// of another whenever an install is half done. Fields move, so a foreign row read
/// field-by-field yields a plausible verdict rather than a refusal. The engine
/// stamps every row, the driver refuses a stamp it does not know, and the mismatch
/// surfaces as [`crate::Error::EvidenceVersion`] instead of a wrong pass.
///
/// Bump this when a row's meaning changes, not when a field is added.
pub const EVIDENCE_VERSION: u64 = 1;

/// How long the engine may spend on one page before reporting what it has.
const BUDGET_MS: u64 = 20_000;

/// Points in one approach path. Enough curvature to carry the corpus shape,
/// short enough that the whole path is one event burst.
const PATH_POINTS: usize = 24;

/// Entries in each dynamics deck.
///
/// The engine deals one per interaction, so this is how many touches a page may
/// take before a shape repeats. A dozen covers a grid solve; the deck is sampled
/// once at launch, so a larger one costs launch time for motion no page uses.
const DECK: usize = 12;

/// What the engine reported for one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOutcome {
    /// Kind the engine classified, from chrome signals.
    pub kind: String,
    /// Vendor binding that matched, if any.
    pub vendor: Option<String>,
    /// Whether the vendor wrote its token.
    pub solved: bool,
    /// How the write was observed: `field`, `cookie`, or a clean document.
    pub via: Option<String>,
    /// Milliseconds the engine spent.
    pub ms: u64,
    /// Typed refusal, when the engine would not claim the kind.
    pub error: Option<String>,
    /// Browsing contexts the observer attached to for this page.
    pub contexts: u64,
    /// Page the row belongs to.
    pub url: String,
}

/// Configuration handed to the engine for one session.
#[derive(Debug, Clone)]
pub struct ChallengeConfig {
    /// Where the engine appends its evidence.
    pub evidence: PathBuf,
    /// Directory holding the challenge modules, for an engine that has them on
    /// disk rather than packaged. `None` uses the packaged modules.
    pub modules: Option<PathBuf>,
    /// Total budget per page.
    pub budget_ms: u64,
    /// Loopback helper for the pixel and audio kinds.
    pub helper: Option<(String, u16)>,
    /// A configuration supplied whole by the caller, passed to the engine
    /// unchanged. A fixture run and a helper-equipped run both need to name their
    /// own catalog, and rebuilding it from parts here would silently drop what
    /// they asked for.
    verbatim: Option<String>,
}

impl ChallengeConfig {
    /// Evidence in the system temp directory, keyed by process, packaged
    /// modules, default budget, no helper. An explicit [`CONFIG_ENV`] in this
    /// process's environment is honored as it stands.
    #[must_use]
    pub fn for_process() -> Self {
        if let Some(raw) = verbatim_config() {
            let evidence = evidence_from(&raw).unwrap_or_else(default_evidence_path);
            return Self {
                evidence,
                modules: None,
                budget_ms: BUDGET_MS,
                helper: None,
                verbatim: Some(raw),
            };
        }
        Self {
            evidence: default_evidence_path(),
            modules: source_modules(),
            budget_ms: BUDGET_MS,
            helper: None,
            verbatim: None,
        }
    }

    /// The value of [`CONFIG_ENV`] for this configuration.
    #[must_use]
    pub fn to_env_value(&self) -> String {
        if let Some(raw) = self.verbatim.as_ref() {
            return with_dynamics(raw);
        }
        let seed = dynamics_seed();
        let mut config = serde_json::json!({
            "catalog": catalog::catalog_json(),
            "evidence": self.evidence.display().to_string(),
            "budget_ms": self.budget_ms,
            "claimed_kinds": CLAIMED_KINDS,
            "dynamics_seed": seed,
            "trajectory_deck": deck(seed, TRAJECTORY_SALT, approach_path),
            "drag_deck": deck(seed, DRAG_SALT, drag_profile),
            "prelude_deck": deck(seed, PRELUDE_SALT, prelude_plan),
        });
        if let Some(dir) = self.modules.as_ref() {
            config["modules"] = serde_json::Value::String(dir.display().to_string());
        }
        if let Some((host, port)) = self.helper.as_ref() {
            config["helper"] = serde_json::json!({ "host": host, "port": port });
        }
        config.to_string()
    }

    /// The environment entry to spawn the engine with.
    #[must_use]
    pub fn env_entry(&self) -> (String, String) {
        (CONFIG_ENV.to_string(), self.to_env_value())
    }
}

/// A caller-supplied configuration, if this process has one.
fn verbatim_config() -> Option<String> {
    let raw = std::env::var(CONFIG_ENV).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(raw)
}

/// The evidence path inside a caller-supplied configuration, so the driver
/// watches the file the engine will write.
fn evidence_from(raw: &str) -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let path = value.get("evidence")?.as_str()?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

/// A caller-supplied configuration, with freshly sampled pointer dynamics filled
/// in where it named none.
///
/// A fixture or a helper-equipped run names its own catalog, not its own mouse.
/// Passing such a config through untouched leaves the engine with no trajectory
/// and no drag profile, and a built-in constant is a signature: every session
/// would move identically. A config that does name its own dynamics keeps them,
/// which is how a test drives a shape the sampler would never produce, and a
/// named shape is never joined by a deck, because the deck would outrank it.
fn with_dynamics(raw: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.to_string();
    };
    let Some(map) = value.as_object_mut() else {
        return raw.to_string();
    };
    let seed = map
        .get("dynamics_seed")
        .and_then(serde_json::Value::as_u64)
        .and_then(|raw| u32::try_from(raw).ok())
        .unwrap_or_else(dynamics_seed);
    if !map.contains_key("trajectory") {
        map.entry("trajectory_deck")
            .or_insert_with(|| deck(seed, TRAJECTORY_SALT, approach_path));
    }
    if !map.contains_key("drag_profile") {
        map.entry("drag_deck")
            .or_insert_with(|| deck(seed, DRAG_SALT, drag_profile));
    }
    if !map.contains_key("prelude") {
        map.entry("prelude_deck")
            .or_insert_with(|| deck(seed, PRELUDE_SALT, prelude_plan));
    }
    map.insert("dynamics_seed".to_string(), seed.into());
    value.to_string()
}

/// One evidence file per process, so two sessions in one process share a log and
/// two processes never fight over one.
fn default_evidence_path() -> PathBuf {
    std::env::temp_dir().join(format!("lurien-challenge-{}.jsonl", std::process::id()))
}

/// A checkout running against an engine built before the challenge modules were
/// packaged can point at the modules in the tree. Absent that, the packaged
/// `resource://lurien-challenge/` is used.
fn source_modules() -> Option<PathBuf> {
    std::env::var_os("LURIEN_CHALLENGE_MODULES").map(PathBuf::from)
}

/// Environment variable naming the dynamics seed, so a run can be replayed.
pub const SEED_ENV: &str = "LURIEN_DYNAMICS_SEED";

/// Salts, so one seed yields three unrelated decks rather than three views of one
/// stream: a session whose drags mirror its approaches is a correlation a vendor
/// can measure across visits.
const TRAJECTORY_SALT: u64 = 0x5f37_2d1b;
const DRAG_SALT: u64 = 0xb1c9_44a7;
const PRELUDE_SALT: u64 = 0x27e6_8f03;

/// The seed the session's dynamics are drawn from.
///
/// Entropy by default: a fixed seed shared by every install would be the same
/// signature a constant path is. [`SEED_ENV`] names one instead, which is how a
/// solve that a vendor scored, or a test that watched two clicks differ, is run
/// again with the same motion.
fn dynamics_seed() -> u32 {
    use rand::Rng;
    if let Ok(raw) = std::env::var(SEED_ENV) {
        if let Ok(seed) = raw.trim().parse::<u32>() {
            return seed;
        }
    }
    rand::rngs::StdRng::from_entropy().gen()
}

/// `DECK` independent samples from one sampler, reproducible from `seed`.
///
/// The driver owns the corpus and the sampler; the engine owns the order, because
/// only the browser knows how many widgets a page held. So every entry is drawn
/// here, at launch, and dealt there, per interaction.
fn deck(
    seed: u32,
    salt: u64,
    sample: fn(&mut rand::rngs::StdRng) -> serde_json::Value,
) -> serde_json::Value {
    let mut rng = rand::rngs::StdRng::seed_from_u64(u64::from(seed) ^ salt);
    serde_json::Value::Array((0..DECK).map(|_| sample(&mut rng)).collect())
}

/// A pointer path in the unit square of the target, from outside the widget to
/// its centre, sampled from the persona's own corpus.
///
/// The engine maps these to the widget's own coordinates. It never generates
/// motion: a browser that invents its own curve would contradict the persona
/// that produced the handshake.
fn approach_path(rng: &mut rand::rngs::StdRng) -> serde_json::Value {
    let sampler = MouseSampler::new();
    let points = sampler.resampled_path(-0.35, -0.28, 0.5, 0.5, PATH_POINTS, 0.004, rng);
    let last = points.len().saturating_sub(1);
    let rows: Vec<serde_json::Value> = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            // Ease out: the approach is quick, the settle is not. A constant gap
            // is the tell that gives a synthetic path away.
            let progress = if last == 0 { 1.0 } else { i as f64 / last as f64 };
            let dt = 6.0 + 14.0 * progress * progress;
            serde_json::json!({ "x": x, "y": y, "dt": dt.round() as u64 })
        })
        .collect();
    serde_json::Value::Array(rows)
}

/// Steps in one drag travel. Enough to carry acceleration and two corrections.
const DRAG_STEPS: usize = 16;

/// How far past the answer a hand goes before correcting back, as a fraction of
/// the travel. Measured overshoot on a short drag sits between two and five
/// percent; the exact value is sampled per drag so two solves never match.
const OVERSHOOT: (f64, f64) = (0.02, 0.05);

/// The travel profile for one drag: fractions of the answer, each with its own
/// dwell and vertical wobble.
///
/// Sampled from the same corpus as the approach path, then given an overshoot and
/// two corrections. A vendor that scores a slider scores the dynamics, not the
/// landing: constant speed in a straight line is the shape that fails, and it is
/// the shape every driver-side `dragAndDrop` produces.
fn drag_profile(rng: &mut rand::rngs::StdRng) -> serde_json::Value {
    use rand::Rng;
    let sampler = MouseSampler::new();
    // A drag runs along its own axis, so the sampled path supplies the timing
    // shape and the wobble; the fraction is its horizontal progress.
    let points = sampler.resampled_path(0.0, 0.0, 1.0, 0.0, DRAG_STEPS, 0.9, rng);
    let last = points.len().saturating_sub(1);
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(DRAG_STEPS + 3);
    for (i, (x, y)) in points.iter().enumerate() {
        let progress = if last == 0 { 1.0 } else { i as f64 / last as f64 };
        // Fast through the middle, slow at both ends: a hand does not start or
        // stop a drag at speed.
        let dt = 9.0 + 16.0 * (progress - 0.5).abs() * 2.0;
        rows.push(serde_json::json!({
            "f": (x.clamp(0.0, 0.985) * 1000.0).round() / 1000.0,
            "dy": (y * 100.0).round() / 100.0,
            "dt": dt.round() as u64,
        }));
    }
    let overshoot = rng.gen_range(OVERSHOOT.0..OVERSHOOT.1);
    let correction = overshoot * rng.gen_range(0.2..0.6);
    rows.push(serde_json::json!({ "f": 1.0 + overshoot, "dy": 0.8, "dt": 27 }));
    rows.push(serde_json::json!({ "f": 1.0 - correction, "dy": 0.0, "dt": 33 }));
    rows.push(serde_json::json!({ "f": 1.0 + correction / 3.0, "dy": -0.4, "dt": 21 }));
    serde_json::Value::Array(rows)
}

/// Points in the pre-touch wander across the viewport.
const WANDER_POINTS: usize = 14;

/// What happens on a page before its widget is touched.
///
/// A solve that begins with the pointer materializing on the checkbox is the
/// strongest tell left once the events themselves are trusted: a scoring vendor
/// weighs the reading that preceded the click, and a page that was never scrolled
/// and never crossed by a pointer has no reading in it. The plan is data, sampled
/// here from the same corpus and the same pacing library as the rest of the
/// session, and executed by the engine in the page's own context.
///
/// `settle_ms` is the pause after load, `scroll` is a wheel session from
/// `guise::human::scroll` where each step is `{delta, mode, lines, dt}` in the
/// wheel device's own units, `wander` is a pointer path in viewport fractions,
/// and `dwell_ms` is the hover before the act.
///
/// The wheel session and the wander come from `rng`; the pauses come from the
/// pacing library's own draw, so a seeded deck replays its paths, not its dwells.
fn prelude_plan(rng: &mut rand::rngs::StdRng) -> serde_json::Value {
    use guise::human::scroll::{HumanScrollConfig, HumanScroller, ScrollBehavior};
    use guise::human::timing::ActionDelay;
    use guise::human::wheel::WheelDevice;
    // Reading is the intent that fits a page holding a challenge: the visitor is
    // there for the content, not skimming for a link.
    let scroller = HumanScroller::new(HumanScrollConfig {
        total_px: 520,
        behavior: ScrollBehavior::Reading,
        flick_count: 3,
        scroll_down: true,
        wheel_device: WheelDevice::MouseWheel,
    });
    let step_px = WheelDevice::MouseWheel.properties().step_px.max(1.0);
    let scroll: Vec<serde_json::Value> = scroller
        .plan(rng)
        .into_iter()
        .map(|step| {
            // A wheel event carries its delta in the units its device reports:
            // lines for a notched wheel, pixels for a trackpad. The engine
            // dispatches what it is given rather than converting, so the
            // conversion lives here, next to the device that defines the step.
            let delta = if step.delta_mode == 1 {
                step.delta_y / step_px
            } else {
                step.delta_y
            };
            serde_json::json!({
                "delta": (delta * 100.0).round() / 100.0,
                "mode": step.delta_mode,
                "lines": delta.trunc() as i64,
                "dt": step.after_ms,
            })
        })
        .collect();
    // Across the viewport, not towards the widget: this is the traffic that
    // happens before the widget is a target at all.
    let sampler = MouseSampler::new();
    let points = sampler.resampled_path(0.08, 0.12, 0.72, 0.66, WANDER_POINTS, 0.05, rng);
    let wander: Vec<serde_json::Value> = points
        .iter()
        .map(|(x, y)| {
            serde_json::json!({
                "x": (x.clamp(0.02, 0.98) * 1000.0).round() / 1000.0,
                "y": (y.clamp(0.02, 0.98) * 1000.0).round() / 1000.0,
                "dt": u64::try_from(ActionDelay::micro().as_millis()).unwrap_or(120),
            })
        })
        .collect();
    serde_json::json!({
        "settle_ms": u64::try_from(ActionDelay::after_page_load().as_millis()).unwrap_or(1_200),
        "scroll": scroll,
        "wander": wander,
        "dwell_ms": u64::try_from(ActionDelay::hover_dwell().as_millis()).unwrap_or(320),
    })
}

/// Where the evidence file ends right now.
///
/// A verdict is only a verdict for the visit that asked for it. The engine
/// appends, one row per page it takes, and two visits to one url in one session
/// are two rows with the same url: a reader that takes the newest matching row
/// returns the previous visit's verdict the moment the second navigation starts,
/// which reports a page as solved while the engine is still clicking it. So a
/// caller marks the file before it navigates and reads only past that mark.
///
/// A byte offset, not a timestamp: this driver can shift the browser's wall clock
/// on request, so a row's `at` is not a reliable ordering, and an append-only file
/// length is.
#[must_use]
pub fn mark(evidence: &Path) -> u64 {
    std::fs::metadata(evidence).map(|m| m.len()).unwrap_or(0)
}

/// What the evidence file says about one visit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No row for this page since the mark: the engine has not reported yet.
    Pending,
    /// The engine reported.
    Reported(EngineOutcome),
    /// A row this build cannot read, so its contents mean nothing here.
    Unreadable {
        /// Schema version the row named, or `0` when it named none.
        found: u64,
    },
}

/// The engine's last word on `url` since `from`.
///
/// Reads the newest matching row appended after the mark. A missing file, an
/// unreadable file, or a file with no row for this page since the mark all mean
/// the same thing: the engine did not report, so the caller must not claim it did.
///
/// A row whose `v` is not [`EVIDENCE_VERSION`] is refused rather than read
/// field-by-field. Fields move between builds, and reading a foreign row with
/// `unwrap_or` turns a build mismatch into a confident `solved:false`, which is
/// the one answer a caller acts on without asking why.
#[must_use]
pub fn verdict(evidence: &Path, url: &str, from: u64) -> Verdict {
    let Ok(text) = std::fs::read_to_string(evidence) else {
        return Verdict::Pending;
    };
    for line in since(&text, from).lines().rev() {
        let Ok(row) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        let seen = row.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        if !(url.is_empty() || same_page(seen, url)) {
            continue;
        }
        let found = version_of(&row);
        if found != EVIDENCE_VERSION {
            return Verdict::Unreadable { found };
        }
        // A diagnostic row carries `event` and no verdict. Reading one as an
        // outcome reports a page as unsolved while the engine is still working on
        // it, and ends the session mid-solve.
        if row.get("event").is_some() {
            continue;
        }
        if let Some(outcome) = parse_row(&row) {
            return Verdict::Reported(outcome);
        }
    }
    Verdict::Pending
}

/// Schema version a row names. A row from a build older than the version itself
/// names none, which is not version 1: it is a row whose shape is unknown.
fn version_of(row: &serde_json::Value) -> u64 {
    row.get("v")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

/// Is the engine solving this page right now?
///
/// The observer appends one `taken` row the moment it owns a page, before any
/// pixel is read. Only the widget's own context can see a cross-origin challenge,
/// so the page probe reports `none` for a page that is being solved; a caller
/// that trusted the probe would return a clean page and tear the session down
/// mid-solve. A `taken` row with no verdict after it means the work is still
/// running.
#[must_use]
pub fn taken(evidence: &Path, url: &str, from: u64) -> bool {
    let Ok(text) = std::fs::read_to_string(evidence) else {
        return false;
    };
    since(&text, from).lines().any(|line| {
        let Ok(row) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            return false;
        };
        if version_of(&row) != EVIDENCE_VERSION {
            return false;
        }
        if row.get("event").and_then(|v| v.as_str()) != Some("taken") {
            return false;
        }
        let seen = row.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        url.is_empty() || same_page(seen, url)
    })
}

/// The part of the file appended after a mark.
///
/// A mark past the end means the file was replaced rather than appended to, and
/// everything in it is newer than the mark, not older.
fn since(text: &str, from: u64) -> &str {
    usize::try_from(from)
        .ok()
        .and_then(|from| text.get(from..))
        .unwrap_or(text)
}

/// Evidence rows carry the URL the engine saw, which may differ from the URL
/// asked for by a redirect or a trailing slash.
fn same_page(seen: &str, asked: &str) -> bool {
    seen == asked || seen.trim_end_matches('/') == asked.trim_end_matches('/')
}

/// A verdict row, once its schema version is one this build reads.
fn parse_row(row: &serde_json::Value) -> Option<EngineOutcome> {
    let solved = row.get("solved")?.as_bool()?;
    let kind = row.get("kind")?.as_str()?.to_string();
    Some(EngineOutcome {
        kind,
        vendor: row
            .get("vendor")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        solved,
        via: row.get("via").and_then(|v| v.as_str()).map(str::to_string),
        ms: row.get("ms").and_then(serde_json::Value::as_u64).unwrap_or(0),
        error: row.get("error").and_then(|v| v.as_str()).map(str::to_string),
        contexts: row
            .get("contexts")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        url: row
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_carries_the_catalog_and_the_claimed_kinds() {
        let config = ChallengeConfig::for_process();
        let value: serde_json::Value =
            serde_json::from_str(&config.to_env_value()).expect("config is json");
        let catalog = value["catalog"].as_array().expect("catalog array");
        assert_eq!(catalog.len(), crate::catalog::CATALOG.len());
        for row in catalog {
            assert!(row["name"].is_string(), "binding has no name: {row}");
            assert!(row["kind"].is_string(), "binding has no kind: {row}");
            assert!(
                !row["target"].as_str().unwrap_or_default().is_empty(),
                "binding has no target, so the engine could not act on it: {row}"
            );
        }
        let claimed: Vec<&str> = value["claimed_kinds"]
            .as_array()
            .expect("claimed array")
            .iter()
            .map(|v| v.as_str().unwrap_or_default())
            .collect();
        assert_eq!(claimed, CLAIMED_KINDS);
    }

    #[test]
    fn every_claimed_kind_is_a_kind_the_catalog_can_present() {
        for kind in CLAIMED_KINDS {
            let known = ["none", "score", "checkbox", "visual", "slider", "audio", "pow", "fail"];
            assert!(known.contains(kind), "{kind} is not a closed kind");
        }
    }

    /// A caller-supplied config names a catalog, not a mouse. Without this the
    /// engine falls back to a frozen built-in profile, so every session drags
    /// identically, which is the exact signature the sampled profile exists to
    /// avoid.
    #[test]
    fn a_caller_supplied_config_gains_dynamics_but_keeps_its_own() {
        let bare = r#"{"catalog":[],"evidence":"/tmp/e.jsonl","budget_ms":1000}"#;
        let filled: serde_json::Value =
            serde_json::from_str(&with_dynamics(bare)).expect("filled config is json");
        let paths = filled["trajectory_deck"].as_array().expect("trajectory deck");
        assert_eq!(paths.len(), DECK);
        assert!(paths.iter().all(|p| p.as_array().is_some_and(|p| p.len() >= 8)));
        let drags = filled["drag_deck"].as_array().expect("drag deck");
        assert_eq!(drags.len(), DECK);
        assert!(drags.iter().all(|p| p.as_array().is_some_and(|p| p.len() >= 12)));
        assert!(filled["dynamics_seed"].as_u64().is_some(), "the engine cannot deal without a seed");
        assert_eq!(filled["budget_ms"], 1000);
        // Two sessions must not share one deck.
        assert_ne!(
            with_dynamics(bare),
            with_dynamics(bare),
            "the fill-in reused one deck across sessions"
        );

        let named = r#"{"catalog":[],"drag_profile":[{"f":1.0,"dy":0,"dt":10}],"trajectory":[{"x":0.5,"y":0.5,"dt":1}]}"#;
        let kept: serde_json::Value =
            serde_json::from_str(&with_dynamics(named)).expect("kept config is json");
        assert_eq!(kept["drag_profile"].as_array().expect("profile").len(), 1);
        assert_eq!(kept["trajectory"].as_array().expect("path").len(), 1);
        // A deck outranks a named shape in the engine, so a named shape gets none.
        assert!(kept.get("trajectory_deck").is_none(), "the named path was outranked by a deck");
        assert!(kept.get("drag_deck").is_none(), "the named profile was outranked by a deck");

        // A config this build cannot parse is still the caller's, and is passed on
        // rather than replaced with one the caller never asked for.
        assert_eq!(with_dynamics("not json"), "not json");
    }

    /// One click per session is the shape every other solver has. A page holding
    /// two widgets, or a grid taking nine cells, must not repeat one path, and the
    /// engine cannot sample: the corpus and the sampler live here.
    #[test]
    fn a_deck_holds_a_distinct_shape_for_every_interaction() {
        for (name, salt, sample) in [
            ("trajectory", TRAJECTORY_SALT, approach_path as fn(&mut _) -> _),
            ("drag", DRAG_SALT, drag_profile as fn(&mut _) -> _),
            ("prelude", PRELUDE_SALT, prelude_plan as fn(&mut _) -> _),
        ] {
            let dealt = deck(19, salt, sample);
            let rows = dealt.as_array().expect("deck array");
            assert_eq!(rows.len(), DECK, "{name} deck is the wrong size");
            for (i, row) in rows.iter().enumerate() {
                for (j, other) in rows.iter().enumerate().skip(i + 1) {
                    assert_ne!(row, other, "{name} deck entries {i} and {j} are one shape");
                }
            }
        }
    }

    /// A solve a vendor scored, or a test that watched two clicks differ, is only
    /// worth anything if it can be run again with the same motion.
    #[test]
    fn one_seed_replays_every_deck() {
        assert_eq!(
            deck(4_242, TRAJECTORY_SALT, approach_path),
            deck(4_242, TRAJECTORY_SALT, approach_path),
            "one seed drew two different decks"
        );
        assert_ne!(
            deck(4_242, TRAJECTORY_SALT, approach_path),
            deck(4_243, TRAJECTORY_SALT, approach_path),
            "two seeds drew one deck"
        );
        // Salted apart: a session whose drags mirror its approaches correlates.
        assert_ne!(
            deck(4_242, TRAJECTORY_SALT, approach_path),
            deck(4_242, DRAG_SALT, approach_path),
            "two decks came off one stream"
        );
    }

    #[test]
    fn the_trajectory_stays_inside_the_target_and_ends_at_its_centre() {
        let path = approach_path(&mut rand::rngs::StdRng::from_entropy());
        let points = path.as_array().expect("trajectory array");
        assert!(points.len() >= 8, "a {}-point path is not a path", points.len());
        let last = points.last().expect("last point");
        assert!((last["x"].as_f64().unwrap() - 0.5).abs() < 0.01);
        assert!((last["y"].as_f64().unwrap() - 0.5).abs() < 0.01);
        let first = points.first().expect("first point");
        assert!(
            first["x"].as_f64().unwrap() < 0.0,
            "the pointer must approach from outside the widget"
        );
        let gaps: Vec<u64> = points
            .iter()
            .map(|p| p["dt"].as_u64().unwrap_or_default())
            .collect();
        assert!(
            gaps.iter().collect::<std::collections::BTreeSet<_>>().len() > 1,
            "a constant inter-event gap is the tell that gives a synthetic path away"
        );
    }

    #[test]
    fn the_drag_profile_overshoots_corrects_and_never_holds_one_speed() {
        let profile = drag_profile(&mut rand::rngs::StdRng::from_entropy());
        let steps = profile.as_array().expect("profile array");
        assert!(steps.len() >= 12, "a {}-step drag is not a drag", steps.len());
        let fractions: Vec<f64> = steps
            .iter()
            .map(|s| s["f"].as_f64().expect("fraction"))
            .collect();
        assert!(
            fractions.iter().any(|f| *f > 1.0),
            "the travel never passes the answer, so it never corrects back"
        );
        let after_overshoot = fractions
            .iter()
            .position(|f| *f > 1.0)
            .expect("an overshoot");
        assert!(
            fractions[after_overshoot..].iter().any(|f| *f < 1.0),
            "the travel overshoots and never comes back: {fractions:?}"
        );
        assert!(
            fractions.iter().all(|f| *f >= 0.0 && *f < 1.2),
            "a travel outside the answer by more than a fifth is not a correction: {fractions:?}"
        );
        let dwells: Vec<u64> = steps
            .iter()
            .map(|s| s["dt"].as_u64().expect("dwell"))
            .collect();
        assert!(
            dwells.iter().collect::<std::collections::BTreeSet<_>>().len() > 2,
            "one dwell for the whole travel is a constant-speed drag: {dwells:?}"
        );
        assert!(
            steps.iter().any(|s| s["dy"].as_f64().expect("wobble").abs() > 0.05),
            "a drag with no vertical wobble is a ruler, not a hand"
        );
    }

    #[test]
    fn two_drags_do_not_share_a_profile() {
        // A profile reused across solves is a signature. Sampling is per drag.
        let mut rng = rand::rngs::StdRng::from_entropy();
        assert_ne!(drag_profile(&mut rng), drag_profile(&mut rng));
    }

    #[test]
    fn the_prelude_reads_the_page_before_it_touches_the_widget() {
        let plan = prelude_plan(&mut rand::rngs::StdRng::from_entropy());
        let settle = plan["settle_ms"].as_u64().expect("settle");
        assert!(
            (300..=4_000).contains(&settle),
            "settling {settle}ms after load is not how a page is read"
        );
        let dwell = plan["dwell_ms"].as_u64().expect("dwell");
        assert!(
            (100..=1_500).contains(&dwell),
            "a {dwell}ms hover before the act is not a hand"
        );

        let scroll = plan["scroll"].as_array().expect("scroll session");
        assert!(scroll.len() >= 3, "a {}-step scroll is not reading", scroll.len());
        let travelled: f64 = scroll.iter().map(|s| s["delta"].as_f64().expect("delta")).sum();
        assert!(travelled > 1.0, "the page was never scrolled down: {travelled}");
        let gaps: std::collections::BTreeSet<u64> =
            scroll.iter().map(|s| s["dt"].as_u64().expect("dt")).collect();
        assert!(gaps.len() > 1, "one wheel cadence for every step is a signature");
        // A notched wheel reports lines, and a wheel event whose delta and its
        // integer line count disagree in sign is not a device report.
        for step in scroll {
            let mode = step["mode"].as_u64().expect("delta mode");
            assert_eq!(mode, 1, "the reading persona is a notched wheel");
            let delta = step["delta"].as_f64().expect("delta");
            let lines = step["lines"].as_i64().expect("line count");
            assert!(
                delta.abs() < 12.0,
                "{delta} lines in one flick is a page jump, not a notch"
            );
            assert!(
                lines == 0 || (lines > 0) == (delta > 0.0),
                "delta {delta} and line count {lines} disagree"
            );
        }

        let wander = plan["wander"].as_array().expect("wander path");
        assert!(wander.len() >= 6, "a {}-point wander is a jump", wander.len());
        let xs: Vec<f64> = wander.iter().map(|p| p["x"].as_f64().expect("x")).collect();
        let ys: Vec<f64> = wander.iter().map(|p| p["y"].as_f64().expect("y")).collect();
        assert!(
            xs.iter().chain(ys.iter()).all(|v| (0.0..=1.0).contains(v)),
            "the wander left the viewport"
        );
        // A path whose every step is the same size is a lerp, which is the shape
        // a driver-side moveTo produces and the shape this plan exists to avoid.
        let steps: Vec<f64> = xs
            .windows(2)
            .zip(ys.windows(2))
            .map(|(x, y)| ((x[1] - x[0]).powi(2) + (y[1] - y[0]).powi(2)).sqrt())
            .collect();
        let mean = steps.iter().sum::<f64>() / steps.len() as f64;
        let spread = steps.iter().map(|s| (s - mean).abs()).fold(0.0, f64::max);
        assert!(spread > mean * 0.05, "the wander moved at one speed: mean {mean}");
    }

    #[test]
    fn two_pages_do_not_share_a_prelude() {
        // Reading behaviour reused across pages is one more constant to match on.
        let mut rng = rand::rngs::StdRng::from_entropy();
        assert_ne!(prelude_plan(&mut rng), prelude_plan(&mut rng));
    }

    #[test]
    fn a_caller_supplied_config_gains_a_prelude_but_keeps_its_own() {
        let bare = r#"{"catalog":[],"budget_ms":1000}"#;
        let filled: serde_json::Value =
            serde_json::from_str(&with_dynamics(bare)).expect("filled config is json");
        let deals = filled["prelude_deck"].as_array().expect("prelude deck");
        assert_eq!(deals.len(), DECK);
        assert!(deals.iter().all(|p| p["scroll"].as_array().is_some_and(|s| !s.is_empty())));

        // An e2e phase proving a page that was never read is refused needs to ship
        // an empty prelude and have it survive.
        let named = r#"{"catalog":[],"prelude":{"settle_ms":0,"scroll":[],"wander":[],"dwell_ms":0}}"#;
        let kept: serde_json::Value =
            serde_json::from_str(&with_dynamics(named)).expect("kept config is json");
        assert_eq!(kept["prelude"]["scroll"].as_array().expect("scroll").len(), 0);
        assert_eq!(kept["prelude"]["settle_ms"], 0);
        assert!(kept.get("prelude_deck").is_none(), "the empty prelude was outranked by a deck");
    }

    /// A row as the engine writes it. Every row it appends carries `v`, so a test
    /// row without one is a row from another build, not a shorthand.
    fn stamped(row: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(row).expect("test row is json");
        value["v"] = EVIDENCE_VERSION.into();
        value.to_string()
    }

    /// The verdict a readable row carries, for tests about content rather than
    /// about versions. An unreadable row is a different failure and never silently
    /// reads as "no verdict yet".
    fn reported(evidence: &Path, url: &str, from: u64) -> Option<EngineOutcome> {
        match verdict(evidence, url, from) {
            Verdict::Reported(outcome) => Some(outcome),
            Verdict::Pending => None,
            Verdict::Unreadable { found } => {
                panic!("a stamped test row read as schema {found}")
            }
        }
    }

    #[test]
    fn an_absent_evidence_file_is_not_a_pass() {
        let missing = std::env::temp_dir().join("lurien-challenge-does-not-exist.jsonl");
        assert_eq!(verdict(&missing, "https://example.com/", 0), Verdict::Pending);
        assert_eq!(mark(&missing), 0);
    }

    #[test]
    fn the_newest_row_for_the_page_wins_and_a_trailing_slash_still_matches() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n",
                stamped(r#"{"kind":"score","solved":false,"ms":1,"url":"https://a.test/","contexts":1}"#),
                stamped(r#"{"kind":"checkbox","vendor":"v","solved":true,"via":"field","ms":900,"url":"https://a.test","contexts":3}"#),
                stamped(r#"{"kind":"none","solved":true,"ms":0,"url":"https://other.test/","contexts":1}"#),
            ),
        )
        .expect("write evidence");
        let found = reported(&path, "https://a.test/", 0).expect("row for the page");
        assert_eq!(found.kind, "checkbox");
        assert!(found.solved);
        assert_eq!(found.via.as_deref(), Some("field"));
        assert_eq!(found.contexts, 3);
        let other = reported(&path, "https://other.test/", 0).expect("row for the other page");
        assert_eq!(other.kind, "none");
        assert_eq!(reported(&path, "https://absent.test/", 0), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_row_is_skipped_rather_than_trusted() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                stamped(r#"{"kind":"score","solved":true,"url":"https://a.test/"}"#),
                r#"{"v":1,"kind":"checkbox","solved":true,"url":"https://a.te"#,
            ),
        )
        .expect("write evidence");
        let found = reported(&path, "https://a.test/", 0).expect("the intact row");
        assert_eq!(found.kind, "score");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Debug builds append one row per sighting so a page the observer never
    /// reached is distinguishable from a page with no challenge. A sighting names
    /// a kind and a url but holds no verdict, and reading one as an outcome ends
    /// the session while the engine is still solving. Every diagnostic row the
    /// engine can emit is checked here, not only the one that caused the fault.
    #[test]
    fn a_diagnostic_row_is_never_read_as_a_verdict() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        let diagnostics = [
            stamped(r#"{"at":"t","event":"configured","bindings":10}"#),
            stamped(r#"{"at":"t","event":"sighting","url":"https://a.test/","top":10,"isTop":true,"kind":"slider","vendor":"fixture","signals":[],"folded":"slider","contexts":2}"#),
            stamped(r#"{"at":"t","event":"sighting","url":"https://a.test/","top":10,"isTop":false,"kind":"checkbox","vendor":"fixture","signals":[],"folded":"checkbox","contexts":2}"#),
        ];
        for row in &diagnostics {
            std::fs::write(&path, format!("{row}\n")).expect("write evidence");
            assert_eq!(
                reported(&path, "https://a.test/", 0),
                None,
                "a diagnostic row was read as a verdict: {row}"
            );
        }
        // A verdict after the diagnostics is still found.
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                diagnostics[1],
                stamped(r#"{"kind":"slider","vendor":"fixture","solved":true,"via":"field","ms":700,"url":"https://a.test/","contexts":2,"source":"engine"}"#)
            ),
        )
        .expect("write evidence");
        let found = reported(&path, "https://a.test/", 0).expect("the verdict row");
        assert!(found.solved);
        assert_eq!(found.kind, "slider");
        // A row with a kind but no verdict field is not a verdict either.
        std::fs::write(
            &path,
            format!("{}\n", stamped(r#"{"kind":"slider","url":"https://a.test/"}"#)),
        )
        .expect("write evidence");
        assert_eq!(reported(&path, "https://a.test/", 0), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The driver and the engine ship as two builds, so a driver reads rows an
    /// older or newer browser wrote whenever an install is half done. Fields move
    /// between versions: a foreign row read field-by-field yields `solved:false`
    /// with a plausible kind, which a caller acts on as a failed challenge instead
    /// of a broken install. Every shape of foreign row is refused here, and the
    /// refusal is a distinct answer from "nothing yet".
    #[test]
    fn a_row_from_another_build_is_refused_rather_than_read() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-ver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        let body = r#""kind":"checkbox","vendor":"v","solved":true,"via":"field","ms":900,"url":"https://a.test/","contexts":2"#;
        let foreign = [
            // A build older than the stamp itself names no version.
            (format!("{{{body}}}"), 0),
            (format!("{{\"v\":99,{body}}}"), 99),
            // A version is a number, not a string or a name.
            (format!("{{\"v\":\"1\",{body}}}"), 0),
        ];
        for (row, found) in &foreign {
            std::fs::write(&path, format!("{row}\n")).expect("write evidence");
            assert_eq!(
                verdict(&path, "https://a.test/", 0),
                Verdict::Unreadable { found: *found },
                "a row from another build was read: {row}"
            );
            assert!(
                !taken(&path, "https://a.test/", 0),
                "a row from another build counted as a running solve: {row}"
            );
        }

        // A row this build wrote is read, and the refusal above was about the
        // stamp rather than the contents.
        std::fs::write(&path, format!("{}\n", stamped(&format!("{{{body}}}")))).expect("write");
        let found = reported(&path, "https://a.test/", 0).expect("the current row");
        assert!(found.solved);

        // A foreign row for another page is not this page's problem.
        std::fs::write(&path, format!("{{{body}}}\n").replace("a.test", "b.test"))
            .expect("write evidence");
        assert_eq!(verdict(&path, "https://a.test/", 0), Verdict::Pending);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two visits to one url in one session write two rows with that url. Without
    /// a mark the second navigation reads the first visit's verdict and reports a
    /// page as solved while the engine is still clicking it, so a caller that
    /// navigates twice is told about a solve that has not happened yet. The same
    /// hole hides a failure: a visit that the engine refuses inherits the earlier
    /// pass.
    #[test]
    fn a_verdict_from_an_earlier_visit_is_not_this_visits_verdict() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-mark-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        let solved = stamped(r#"{"kind":"checkbox","vendor":"v","solved":true,"via":"field","ms":900,"url":"https://a.test/","contexts":2}"#);
        let took = stamped(r#"{"event":"taken","url":"https://a.test/"}"#);
        std::fs::write(&path, format!("{took}\n{solved}\n")).expect("write evidence");

        // The first visit's own reader, marked before its own row existed, sees it.
        assert!(reported(&path, "https://a.test/", 0).is_some());
        assert!(taken(&path, "https://a.test/", 0));

        // A second navigation marks the file first, so the same rows are history.
        let mark = mark(&path);
        assert_eq!(reported(&path, "https://a.test/", mark), None);
        assert!(
            !taken(&path, "https://a.test/", mark),
            "an old taken row made the second visit look like a running solve"
        );

        // What the engine appends after the mark is this visit's, verdict or not.
        let refused = stamped(r#"{"kind":"checkbox","vendor":"v","solved":false,"via":null,"ms":40,"url":"https://a.test/","contexts":2,"error":"refused"}"#);
        std::fs::write(&path, format!("{took}\n{solved}\n{took}\n{refused}\n"))
            .expect("append evidence");
        assert!(taken(&path, "https://a.test/", mark));
        let now = reported(&path, "https://a.test/", mark).expect("this visit's row");
        assert!(!now.solved, "the second visit inherited the first one's pass");
        assert_eq!(now.error.as_deref(), Some("refused"));

        // An evidence file replaced under the caller is read whole rather than
        // skipped: a mark past the end means none of it is history.
        std::fs::write(&path, format!("{solved}\n")).expect("shorter evidence");
        assert!(reported(&path, "https://a.test/", mark).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
