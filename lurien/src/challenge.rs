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
//! - the pointer trajectory, sampled from the same persona corpus as the rest of
//!   the session, so a click matches the identity that made the TLS handshake,
//!   and the kinds this build is allowed to act on,
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
pub const CLAIMED_KINDS: &[&str] = &["none", "score", "checkbox", "fail"];

/// How long the engine may spend on one page before reporting what it has.
const BUDGET_MS: u64 = 20_000;

/// Points in one approach path. Enough curvature to carry the corpus shape,
/// short enough that the whole path is one event burst.
const PATH_POINTS: usize = 24;

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
            return raw.clone();
        }
        let mut config = serde_json::json!({
            "catalog": catalog::catalog_json(),
            "evidence": self.evidence.display().to_string(),
            "budget_ms": self.budget_ms,
            "claimed_kinds": CLAIMED_KINDS,
            "trajectory": approach_path(),
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

/// A pointer path in the unit square of the target, from outside the widget to
/// its centre, sampled from the persona's own corpus.
///
/// The engine maps these to the widget's own coordinates. It never generates
/// motion: a browser that invents its own curve would contradict the persona
/// that produced the handshake.
fn approach_path() -> serde_json::Value {
    let sampler = MouseSampler::new();
    let mut rng = rand::rngs::StdRng::from_entropy();
    let points = sampler.resampled_path(-0.35, -0.28, 0.5, 0.5, PATH_POINTS, 0.004, &mut rng);
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

/// The engine's last word on `url`, if it wrote one.
///
/// Reads the newest matching row. A missing file, an unreadable file, or a file
/// with no row for this page all mean the same thing: the engine did not report,
/// so the caller must not claim it did.
#[must_use]
pub fn outcome_for(evidence: &Path, url: &str) -> Option<EngineOutcome> {
    let text = std::fs::read_to_string(evidence).ok()?;
    text.lines()
        .rev()
        .filter_map(|line| parse_row(line))
        .find(|row| url.is_empty() || same_page(&row.url, url))
}

/// Evidence rows carry the URL the engine saw, which may differ from the URL
/// asked for by a redirect or a trailing slash.
fn same_page(seen: &str, asked: &str) -> bool {
    seen == asked || seen.trim_end_matches('/') == asked.trim_end_matches('/')
}

fn parse_row(line: &str) -> Option<EngineOutcome> {
    let row: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let kind = row.get("kind")?.as_str()?.to_string();
    Some(EngineOutcome {
        kind,
        vendor: row
            .get("vendor")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        solved: row.get("solved").and_then(serde_json::Value::as_bool) == Some(true),
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

    #[test]
    fn the_trajectory_stays_inside_the_target_and_ends_at_its_centre() {
        let path = approach_path();
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
    fn an_absent_evidence_file_is_not_a_pass() {
        let missing = std::env::temp_dir().join("lurien-challenge-does-not-exist.jsonl");
        assert_eq!(outcome_for(&missing, "https://example.com/"), None);
    }

    #[test]
    fn the_newest_row_for_the_page_wins_and_a_trailing_slash_still_matches() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"kind":"score","solved":false,"ms":1,"url":"https://a.test/","contexts":1}"#,
                "\n",
                r#"{"kind":"checkbox","vendor":"v","solved":true,"via":"field","ms":900,"url":"https://a.test","contexts":3}"#,
                "\n",
                r#"{"kind":"none","solved":true,"ms":0,"url":"https://other.test/","contexts":1}"#,
                "\n",
            ),
        )
        .expect("write evidence");
        let found = outcome_for(&path, "https://a.test/").expect("row for the page");
        assert_eq!(found.kind, "checkbox");
        assert!(found.solved);
        assert_eq!(found.via.as_deref(), Some("field"));
        assert_eq!(found.contexts, 3);
        let other = outcome_for(&path, "https://other.test/").expect("row for the other page");
        assert_eq!(other.kind, "none");
        assert_eq!(outcome_for(&path, "https://absent.test/"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_row_is_skipped_rather_than_trusted() {
        let dir = std::env::temp_dir().join(format!("lurien-evidence-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("evidence.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"kind":"score","solved":true,"url":"https://a.test/"}"#,
                "\n",
                r#"{"kind":"checkbox","solved":true,"url":"https://a.te"#,
                "\n",
            ),
        )
        .expect("write evidence");
        let found = outcome_for(&path, "https://a.test/").expect("the intact row");
        assert_eq!(found.kind, "score");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
