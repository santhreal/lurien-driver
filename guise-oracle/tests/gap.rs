//! Gap tests: deliberate behaviors and documented limitations of the
//! guise-oracle taxonomy that must stay pinned so any future change is a
//! conscious decision, not silent drift.

use guise_oracle::{Divergence, DivergenceKind, Severity, ThreeWayReport};

/// GAP: a `Divergence` serialized before the `kind` field existed (older
/// fixture files on disk) deserializes with `kind = EngineDivergence`, the
/// conservative default that routes the surface to human triage. Changing
/// the default to `PersonaIntended` would silently reclassify historical
/// engine divergences as expected persona differences.
#[test]
fn divergence_without_kind_field_defaults_to_engine_divergence() {
    let legacy = r#"{
        "surface": "webgl.unmasked_renderer",
        "severity": "High",
        "a_value": "a",
        "b_value": "b"
    }"#;
    let divergence: Divergence = serde_json::from_str(legacy).expect("legacy fixture parses");
    assert_eq!(divergence.kind, DivergenceKind::EngineDivergence);
    assert_eq!(divergence.surface_id, None);
}

/// GAP: `engine_better_than_js` is strict inequality, so a report with zero
/// wins on both sides (no diverging surfaces at all, or only
/// `everyone_loses`) reports `false`. "Not worse" is not "better": an empty
/// comparison carries no evidence that the engine patch helps.
#[test]
fn engine_better_than_js_is_false_with_no_evidence() {
    let report = ThreeWayReport {
        surfaces: 0,
        engine_wins: Vec::new(),
        js_wins: Vec::new(),
        everyone_loses: Vec::new(),
    };
    assert!(!report.engine_better_than_js());
    assert!(report.summary().contains("0 engine wins"));
}

/// GAP: `severity_rank` is the ONLY ordering authority for `Severity`; the
/// enum deliberately does not derive `Ord` so a maintainer reordering the
/// variants cannot silently change report sorting. Pinned: High > Medium >
/// Low by rank.
#[test]
fn severity_ordering_lives_only_in_severity_rank() {
    use guise_oracle::severity_rank;
    assert!(severity_rank(&Severity::High) > severity_rank(&Severity::Medium));
    assert!(severity_rank(&Severity::Medium) > severity_rank(&Severity::Low));
}
