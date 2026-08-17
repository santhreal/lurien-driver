//! Shared surface-taxonomy data types for the guise differential oracle.
//!
//! This crate deliberately contains only the **contract types**: severities,
//! probe definitions, captured values, and differential-report structures, so
//! consumers such as `lurien`, `sear`, and `guise` can share one
//! fingerprint taxonomy without pulling in a browser driver, TLS stack, or
//! behavioral model.  All runtime evaluation and rendering logic lives in the
//! `guise` crate, which re-exports these types from its `probe` module.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Severity of a drift.  `Low` won't fail CI; `High` should.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Cosmetic (does not by itself reveal automation).
    Low,
    /// Notable (contributes to a detection score).
    Medium,
    /// Strong (a fingerprinter weights this heavily).
    High,
}

impl Severity {
    /// String representation of severity (`"Low"`, `"Medium"`, `"High"`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Low => "Low",
            Severity::Medium => "Medium",
            Severity::High => "High",
        }
    }

    /// Parse a string (`"Low"`, `"Medium"`, `"High"`, case-insensitive) into a [`Severity`].
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Severity::Low),
            "medium" => Some(Severity::Medium),
            "high" => Some(Severity::High),
            _ => None,
        }
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or_else(|| format!("invalid severity string: '{s}'"))
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a probe's raw value is reproducible across runs.  Drives how the
/// differential oracle compares two browsers on that surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Determinism {
    /// Same browser ⇒ same value.  Compared by raw value.
    Deterministic,
    /// Value varies run-to-run (timer entropy, scheduler jitter, history depth).
    /// Compared by classified outcome class so noise is not a tell.
    Stochastic,
}
impl Determinism {
    /// String representation (`"Deterministic"`, `"Stochastic"`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Determinism::Deterministic => "Deterministic",
            Determinism::Stochastic => "Stochastic",
        }
    }

    /// Parse a string (`"Deterministic"`, `"Stochastic"`, case-insensitive).
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "deterministic" => Some(Determinism::Deterministic),
            "stochastic" => Some(Determinism::Stochastic),
            _ => None,
        }
    }
}

impl std::fmt::Display for Determinism {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Determinism {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or_else(|| format!("invalid determinism string: '{s}'"))
    }
}

/// Outcome of a single probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeOutcome {
    /// Surface looks like a real browser.
    Pass,
    /// Surface differs from a real browser but the difference is minor.
    Drift(String),
    /// Surface has a critical leak.
    Critical(String),
    /// The probe failed to execute.
    ProbeError(String),
}

impl ProbeOutcome {
    /// `true` if the surface looked like a real browser.
    #[must_use]
    pub fn is_pass(&self) -> bool {
        matches!(self, ProbeOutcome::Pass)
    }

    /// `true` if the surface had a critical (automation-revealing) leak.
    #[must_use]
    pub fn is_critical(&self) -> bool {
        matches!(self, ProbeOutcome::Critical(_))
    }
    /// `true` if the surface had a minor difference (non-critical drift).
    #[must_use]
    pub fn is_drift(&self) -> bool {
        matches!(self, ProbeOutcome::Drift(_))
    }

    /// `true` if the probe failed to execute.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, ProbeOutcome::ProbeError(_))
    }

    /// Payload message for non-pass outcomes (`Drift`, `Critical`, `ProbeError`).
    /// Returns `None` for `Pass`.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match self {
            ProbeOutcome::Pass => None,
            ProbeOutcome::Drift(msg)
            | ProbeOutcome::Critical(msg)
            | ProbeOutcome::ProbeError(msg) => Some(msg.as_str()),
        }
    }

    /// Stable class label ignoring the payload string. `"Pass"`, `"Drift"`,
    /// `"Critical"`, `"ProbeError"`.  The differential oracle compares stochastic
    /// surfaces by this label so two healthy-but-numerically-different runs agree.
    #[must_use]
    pub fn class_label(&self) -> &'static str {
        match self {
            ProbeOutcome::Pass => "Pass",
            ProbeOutcome::Drift(_) => "Drift",
            ProbeOutcome::Critical(_) => "Critical",
            ProbeOutcome::ProbeError(_) => "ProbeError",
        }
    }
}
impl std::fmt::Display for ProbeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeOutcome::Pass => f.write_str("Pass"),
            ProbeOutcome::Drift(msg) => write!(f, "Drift: {msg}"),
            ProbeOutcome::Critical(msg) => write!(f, "Critical: {msg}"),
            ProbeOutcome::ProbeError(msg) => write!(f, "ProbeError: {msg}"),
        }
    }
}

/// One surface probe: a name, JS expression, and a predicate that classifies the
/// returned `serde_json::Value` as `Pass` / `Drift` / `Critical`.
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    /// Human-readable name of the surface this probe checks.
    pub name: &'static str,
    /// JS expression evaluated on the page; its return value is classified.
    pub js: &'static str,
    /// How badly a failure of this probe betrays automation.
    pub severity: Severity,
    /// Maps the probe's returned `serde_json::Value` to a [`ProbeOutcome`].
    pub classifier: fn(&serde_json::Value) -> ProbeOutcome,
    /// Whether this surface is reproducible run-to-run.
    pub determinism: Determinism,
}
impl Probe {
    /// Classify a JSON value returned by JS evaluation against this probe's predicate.
    #[must_use]
    pub fn run_classifier(&self, value: &serde_json::Value) -> ProbeOutcome {
        (self.classifier)(value)
    }
}

/// Why a differential-oracle surface diverged between two browsers.  This makes
/// mechanical the triage between "intended persona-vs-host" and "unexplained
/// engine difference".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DivergenceKind {
    /// The diverging surface is one the persona deliberately overrides, so a
    /// difference is expected rather than an engine-level tell.
    PersonaIntended,
    /// The surface is not persona-overridden, so a divergence is an engine-level
    /// difference to investigate.  This is the default when deserializing an
    /// older report that did not store the field.
    #[default]
    EngineDivergence,
}
impl DivergenceKind {
    /// String representation (`"PersonaIntended"`, `"EngineDivergence"`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            DivergenceKind::PersonaIntended => "PersonaIntended",
            DivergenceKind::EngineDivergence => "EngineDivergence",
        }
    }

    /// Parse a string (`"PersonaIntended"`, `"EngineDivergence"`, case-insensitive).
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "personaintended" | "persona_intended" | "persona-intended" => {
                Some(DivergenceKind::PersonaIntended)
            }
            "enginedivergence" | "engine_divergence" | "engine-divergence" => {
                Some(DivergenceKind::EngineDivergence)
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for DivergenceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DivergenceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or_else(|| format!("invalid divergence kind string: '{s}'"))
    }
}

/// One surface where two browsers returned different values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Divergence {
    /// Surface name from the catalogue (e.g. `"webgl.unmasked_renderer"`).
    pub surface: String,
    /// Shared taxonomy surface-id when the probe is bridged to an inventory.
    #[serde(default)]
    pub surface_id: Option<String>,
    /// How heavily a fingerprinter weights this surface.
    pub severity: Severity,
    /// Why it diverged: persona-intended vs engine-level.
    #[serde(default)]
    pub kind: DivergenceKind,
    /// Value the first browser (`label_a`) returned, rendered as compact JSON.
    pub a_value: String,
    /// Value the second browser (`label_b`) returned, rendered as compact JSON.
    pub b_value: String,
}

/// Result of diffing two browsers across the surface catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifferentialReport {
    /// Label of the first browser (e.g. `"stock-firefox"`).
    pub label_a: String,
    /// Label of the second browser (e.g. `"lurien"` or `"disguise"`).
    pub label_b: String,
    /// Surfaces compared.
    pub surfaces: usize,
    /// Surfaces where both browsers returned the same value.
    pub agreed: usize,
    /// Surfaces where the values diverged.
    pub diverged: usize,
    /// Surfaces where at least one browser failed to evaluate the probe.
    pub errors: usize,
    /// Every divergence, sorted High → Low severity then by name.
    pub divergences: Vec<Divergence>,
}

impl DifferentialReport {
    /// `true` when the two browsers are fingerprint-identical across the whole
    /// catalogue.
    #[must_use]
    pub fn is_identical(&self) -> bool {
        self.diverged == 0 && self.errors == 0
    }

    /// The highest severity among the divergences, if any.
    #[must_use]
    pub fn worst(&self) -> Option<Severity> {
        self.divergences
            .iter()
            .map(|d| d.severity)
            .max_by_key(severity_rank)
    }

    /// The highest severity among engine-level (non-persona-intended) divergences.
    #[must_use]
    pub fn engine_worst(&self) -> Option<Severity> {
        self.engine_divergences()
            .map(|d| d.severity)
            .max_by_key(severity_rank)
    }

    /// Divergences NOT explained by a persona override, the engine-level
    /// differences a caller must triage.
    pub fn engine_divergences(&self) -> impl Iterator<Item = &Divergence> {
        self.divergences
            .iter()
            .filter(|d| d.kind == DivergenceKind::EngineDivergence)
    }

    /// Count of engine-level (non-persona-intended) divergences.
    #[must_use]
    pub fn engine_divergence_count(&self) -> usize {
        self.engine_divergences().count()
    }
    /// Divergences explained by an intended persona override.
    pub fn persona_divergences(&self) -> impl Iterator<Item = &Divergence> {
        self.divergences
            .iter()
            .filter(|d| d.kind == DivergenceKind::PersonaIntended)
    }

    /// Count of persona-intended divergences.
    #[must_use]
    pub fn persona_divergence_count(&self) -> usize {
        self.persona_divergences().count()
    }

    /// `true` if internal counts (`agreed + diverged + errors == surfaces`) and
    /// `divergences.len() == diverged`.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.agreed
            .saturating_add(self.diverged)
            .saturating_add(self.errors)
            == self.surfaces
            && self.divergences.len() == self.diverged
    }

    /// One-line human summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} vs {}: {}/{} surfaces agree ({} diverged, {} errors){}",
            self.label_a,
            self.label_b,
            self.agreed,
            self.surfaces,
            self.diverged,
            self.errors,
            match self.worst() {
                Some(s) => format!(", worst={s:?}"),
                None => String::new(),
            }
        )
    }
}

/// Result of running every probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    /// Total probes run.
    pub probed: usize,
    /// Probes that matched a real browser.
    pub passed: usize,
    /// Probes with a minor (non-critical) difference.
    pub drift: usize,
    /// Probes with an automation-revealing leak.
    pub critical: usize,
    /// Probes that failed to execute.
    pub probe_errors: usize,
    /// Per-probe detail.
    pub per_probe: Vec<ProbeReport>,
}

impl DriftReport {
    /// `true` when there are no criticals and >=90% of probes pass.
    ///
    /// The percentage is exact: computed in `u128` so a small report cannot
    /// round the threshold down (2/3 passing is 66%, not green) and a hostile
    /// deserialized report with counts near `usize::MAX` cannot overflow the
    /// multiplication.
    #[must_use]
    pub fn is_green(&self) -> bool {
        self.critical == 0
            && self.probed > 0
            && (self.passed as u128) * 100 >= (self.probed as u128) * 90
    }

    /// `true` if `passed + drift + critical + probe_errors == probed` and
    /// `per_probe` is either empty or has length `probed` with matching per-outcome counts.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let counts_sum = self
            .passed
            .saturating_add(self.drift)
            .saturating_add(self.critical)
            .saturating_add(self.probe_errors)
            == self.probed;
        if !counts_sum {
            return false;
        }
        if self.per_probe.is_empty() {
            return true;
        }
        if self.per_probe.len() != self.probed {
            return false;
        }

        let mut actual_passed = 0usize;
        let mut actual_drift = 0usize;
        let mut actual_critical = 0usize;
        let mut actual_errors = 0usize;

        for p in &self.per_probe {
            match p.outcome {
                ProbeOutcome::Pass => actual_passed += 1,
                ProbeOutcome::Drift(_) => actual_drift += 1,
                ProbeOutcome::Critical(_) => actual_critical += 1,
                ProbeOutcome::ProbeError(_) => actual_errors += 1,
            }
        }

        actual_passed == self.passed
            && actual_drift == self.drift
            && actual_critical == self.critical
            && actual_errors == self.probe_errors
    }

    /// One-line human summary of the report.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{}/{} probes pass ({} drift, {} critical, {} errors)",
            self.passed, self.probed, self.drift, self.critical, self.probe_errors
        )
    }
}

/// Per-probe entry in the drift report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeReport {
    /// The probe's name.
    pub name: String,
    /// The probe's severity, rendered as a string for serialization.
    pub severity: String,
    /// The classified outcome.
    pub outcome: ProbeOutcome,
}
impl ProbeReport {
    /// Parse the string severity into a [`Severity`].
    #[must_use]
    pub fn severity_enum(&self) -> Option<Severity> {
        Severity::from_str(&self.severity)
    }
}

/// Numeric rank for severity ordering (High > Medium > Low).
#[must_use]
pub fn severity_rank(s: &Severity) -> u8 {
    match s {
        Severity::Low => 0,
        Severity::Medium => 1,
        Severity::High => 2,
    }
}

/// One captured surface value from a browser, suitable for serializing as an
/// offline fixture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedSurface {
    /// Catalogue probe name.
    pub name: String,
    /// Probe severity.
    pub severity: Severity,
    /// Rendered JSON value, or an error message if the probe failed.
    pub value: Result<String, String>,
}

/// A full set of probe results captured from one browser.  This is the offline
/// fixture format: it can be written to JSON and diffed later without a live
/// browser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capture {
    /// Human label for the captured browser (e.g. `"stock-firefox-151"`).
    pub label: String,
    /// Captured surfaces, in catalogue order.
    pub surfaces: Vec<CapturedSurface>,
}
impl Capture {
    /// `true` if this capture has a non-empty label and no duplicate surface names.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        !self.label.trim().is_empty() && !self.has_duplicate_surfaces()
    }

    /// `true` if `surfaces` contains duplicate surface names.
    #[must_use]
    pub fn has_duplicate_surfaces(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        for s in &self.surfaces {
            if !seen.insert(s.name.as_str()) {
                return true;
            }
        }
        false
    }
    /// Build a name-indexed map for diffing.
    #[must_use]
    pub fn by_name(&self) -> BTreeMap<&str, (&Severity, &Result<String, String>)> {
        self.surfaces
            .iter()
            .map(|s| (s.name.as_str(), (&s.severity, &s.value)))
            .collect()
    }

    /// Compare two captured browser fixtures to produce a [`DifferentialReport`].
    ///
    /// Performs pure offline diffing without a live browser or JS runtime.
    /// Probes present in both captures are compared: if both values are `Ok` and
    /// identical, they agree; if values differ, a [`Divergence`] is recorded
    /// with severity from the surface; if either probe returned an error, an error
    /// is counted.
    #[must_use]
    pub fn diff(&self, other: &Capture) -> DifferentialReport {
        let a_map = self.by_name();
        let b_map = other.by_name();

        let mut agreed = 0;
        let mut diverged = 0;
        let mut errors = 0;
        let mut divergences = Vec::new();

        let mut all_keys: Vec<&str> = a_map.keys().copied().chain(b_map.keys().copied()).collect();
        all_keys.sort_unstable();
        all_keys.dedup();

        let total_surfaces = all_keys.len();

        for name in all_keys {
            match (a_map.get(name), b_map.get(name)) {
                (Some((sev_a, Ok(val_a))), Some((sev_b, Ok(val_b)))) => {
                    if val_a == val_b {
                        agreed += 1;
                    } else {
                        diverged += 1;
                        let max_sev = if severity_rank(sev_a) >= severity_rank(sev_b) {
                            **sev_a
                        } else {
                            **sev_b
                        };
                        divergences.push(Divergence {
                            surface: (*name).to_string(),
                            surface_id: Some((*name).to_string()),
                            severity: max_sev,
                            kind: DivergenceKind::EngineDivergence,
                            a_value: (*val_a).clone(),
                            b_value: (*val_b).clone(),
                        });
                    }
                }
                (Some((sev_a, Ok(val_a))), None) => {
                    diverged += 1;
                    divergences.push(Divergence {
                        surface: (*name).to_string(),
                        surface_id: Some((*name).to_string()),
                        severity: **sev_a,
                        kind: DivergenceKind::EngineDivergence,
                        a_value: (*val_a).clone(),
                        b_value: "<missing>".to_string(),
                    });
                }
                (None, Some((sev_b, Ok(val_b)))) => {
                    diverged += 1;
                    divergences.push(Divergence {
                        surface: (*name).to_string(),
                        surface_id: Some((*name).to_string()),
                        severity: **sev_b,
                        kind: DivergenceKind::EngineDivergence,
                        a_value: "<missing>".to_string(),
                        b_value: (*val_b).clone(),
                    });
                }
                _ => {
                    errors += 1;
                }
            }
        }

        divergences.sort_by(|d1, d2| {
            severity_rank(&d2.severity)
                .cmp(&severity_rank(&d1.severity))
                .then_with(|| d1.surface.cmp(&d2.surface))
        });

        DifferentialReport {
            label_a: self.label.clone(),
            label_b: other.label.clone(),
            surfaces: total_surfaces,
            agreed,
            diverged,
            errors,
            divergences,
        }
    }
}

/// One surface in a three-way comparison (stock vs lurien vs JS-disguise).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreeWaySurface {
    /// Catalogue probe name.
    pub surface: String,
    /// Shared taxonomy surface-id when bridged.
    pub surface_id: Option<String>,
    /// Probe severity.
    pub severity: Severity,
    /// Value from the stock browser.
    pub stock_value: String,
    /// Value from the patched lurien engine (`lurien_value` is the serialized name).
    pub lurien_value: String,
    /// Value from the JS disguise.
    pub disguise_value: String,
}

/// Result of comparing stock Firefox, patched lurien, and the JS disguise
/// across the surface catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreeWayReport {
    /// Surfaces compared across all three captures.
    pub surfaces: usize,
    /// Surfaces where stock == lurien != disguise.
    pub engine_wins: Vec<ThreeWaySurface>,
    /// Surfaces where stock == disguise != lurien.
    pub js_wins: Vec<ThreeWaySurface>,
    /// Surfaces where all three differ.
    pub everyone_loses: Vec<ThreeWaySurface>,
}

impl ThreeWayReport {
    /// `true` when the engine patch is strictly closer to stock than the JS
    /// disguise.
    #[must_use]
    pub fn engine_better_than_js(&self) -> bool {
        self.engine_wins.len() > self.js_wins.len()
    }

    /// Count of surfaces where all three captures (stock, lurien, disguise) agree.
    #[must_use]
    pub fn agreed_count(&self) -> usize {
        self.surfaces
            .saturating_sub(self.engine_wins.len())
            .saturating_sub(self.js_wins.len())
            .saturating_sub(self.everyone_loses.len())
    }

    /// `true` if total recorded win/loss categories do not exceed total surfaces.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.engine_wins
            .len()
            .saturating_add(self.js_wins.len())
            .saturating_add(self.everyone_loses.len())
            <= self.surfaces
    }
    /// `true` when stock, lurien engine, and JS disguise agree across all surfaces.
    #[must_use]
    pub fn all_agree(&self) -> bool {
        self.engine_wins.is_empty() && self.js_wins.is_empty() && self.everyone_loses.is_empty()
    }

    /// Compare three captured browser fixtures (stock Firefox, patched lurien, JS disguise)
    /// to produce a [`ThreeWayReport`].
    #[must_use]
    pub fn from_captures(stock: &Capture, lurien: &Capture, disguise: &Capture) -> ThreeWayReport {
        let stock_map = stock.by_name();
        let lurien_map = lurien.by_name();
        let disguise_map = disguise.by_name();

        let mut all_keys: Vec<&str> = stock_map
            .keys()
            .copied()
            .chain(lurien_map.keys().copied())
            .chain(disguise_map.keys().copied())
            .collect();
        all_keys.sort_unstable();
        all_keys.dedup();

        let total_surfaces = all_keys.len();
        let mut engine_wins = Vec::new();
        let mut js_wins = Vec::new();
        let mut everyone_loses = Vec::new();

        for name in all_keys {
            let (stock_sev, stock_val) = match stock_map.get(name) {
                Some((sev, Ok(v))) => (*sev, v.clone()),
                Some((sev, Err(e))) => (*sev, format!("<error: {e}>")),
                None => (&Severity::Low, "<missing>".to_string()),
            };
            let (lurien_sev, lurien_val) = match lurien_map.get(name) {
                Some((sev, Ok(v))) => (*sev, v.clone()),
                Some((sev, Err(e))) => (*sev, format!("<error: {e}>")),
                None => (&Severity::Low, "<missing>".to_string()),
            };
            let (disguise_sev, disguise_val) = match disguise_map.get(name) {
                Some((sev, Ok(v))) => (*sev, v.clone()),
                Some((sev, Err(e))) => (*sev, format!("<error: {e}>")),
                None => (&Severity::Low, "<missing>".to_string()),
            };

            let max_sev = *[stock_sev, lurien_sev, disguise_sev]
                .into_iter()
                .max_by_key(|s| severity_rank(s))
                .unwrap_or(&Severity::Low);

            let surface_entry = ThreeWaySurface {
                surface: (*name).to_string(),
                surface_id: Some((*name).to_string()),
                severity: max_sev,
                stock_value: stock_val.clone(),
                lurien_value: lurien_val.clone(),
                disguise_value: disguise_val.clone(),
            };

            if stock_val == lurien_val && stock_val != disguise_val {
                engine_wins.push(surface_entry);
            } else if stock_val == disguise_val && stock_val != lurien_val {
                js_wins.push(surface_entry);
            } else if stock_val != lurien_val && stock_val != disguise_val {
                everyone_loses.push(surface_entry);
            }
        }

        ThreeWayReport {
            surfaces: total_surfaces,
            engine_wins,
            js_wins,
            everyone_loses,
        }
    }

    /// Human summary of the three-way comparison.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "three-way over {} surfaces: {} engine wins, {} js wins, {} everyone loses",
            self.surfaces,
            self.engine_wins.len(),
            self.js_wins.len(),
            self.everyone_loses.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_rank_orders_high_above_low() {
        assert!(severity_rank(&Severity::High) > severity_rank(&Severity::Low));
        assert!(severity_rank(&Severity::High) > severity_rank(&Severity::Medium));
        assert!(severity_rank(&Severity::Medium) > severity_rank(&Severity::Low));
    }

    #[test]
    fn probe_outcome_class_labels_are_stable() {
        assert_eq!(ProbeOutcome::Pass.class_label(), "Pass");
        assert_eq!(ProbeOutcome::Drift("x".into()).class_label(), "Drift");
        assert_eq!(ProbeOutcome::Critical("x".into()).class_label(), "Critical");
        assert_eq!(
            ProbeOutcome::ProbeError("x".into()).class_label(),
            "ProbeError"
        );
    }

    #[test]
    fn differential_report_serializes_and_deserializes() {
        let report = DifferentialReport {
            label_a: "stock".into(),
            label_b: "disguise".into(),
            surfaces: 10,
            agreed: 9,
            diverged: 1,
            errors: 0,
            divergences: vec![Divergence {
                surface: "navigator.webdriver".into(),
                surface_id: Some("navigator.webdriver".into()),
                severity: Severity::High,
                kind: DivergenceKind::EngineDivergence,
                a_value: "false".into(),
                b_value: "true".into(),
            }],
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: DifferentialReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.label_a, report.label_a);
        assert_eq!(back.label_b, report.label_b);
        assert_eq!(back.surfaces, report.surfaces);
        assert_eq!(back.divergences.len(), report.divergences.len());
        assert_eq!(back.divergences[0].surface, report.divergences[0].surface);
    }

    #[test]
    fn drift_report_is_green_when_ninety_percent_pass_and_no_criticals() {
        let report = DriftReport {
            probed: 10,
            passed: 9,
            drift: 1,
            critical: 0,
            probe_errors: 0,
            per_probe: Vec::new(),
        };
        assert!(report.is_green());
        let not_green = DriftReport {
            probed: 10,
            passed: 10,
            drift: 0,
            critical: 1,
            probe_errors: 0,
            per_probe: Vec::new(),
        };
        assert!(!not_green.is_green());
    }
}
