//! Property-based tests for guise-oracle.
//!
//! These pin the serialization and classification invariants every consumer
//! of the shared surface taxonomy (`lurien`, `sear`, `guise`) relies
//! on, for arbitrary report contents, not just hand-written fixtures.

use guise_oracle::{
    severity_rank, Capture, CapturedSurface, DifferentialReport, Divergence, DivergenceKind,
    ProbeOutcome, Severity,
};
use proptest::prelude::*;

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Low),
        Just(Severity::Medium),
        Just(Severity::High),
    ]
}

fn arb_outcome() -> impl Strategy<Value = ProbeOutcome> {
    prop_oneof![
        Just(ProbeOutcome::Pass),
        ".*".prop_map(ProbeOutcome::Drift),
        ".*".prop_map(ProbeOutcome::Critical),
        ".*".prop_map(ProbeOutcome::ProbeError),
    ]
}

fn arb_divergence() -> impl Strategy<Value = Divergence> {
    (
        ".*",
        prop::option::of(".*"),
        arb_severity(),
        prop_oneof![
            Just(DivergenceKind::PersonaIntended),
            Just(DivergenceKind::EngineDivergence),
        ],
        ".*",
        ".*",
    )
        .prop_map(
            |(surface, surface_id, severity, kind, a_value, b_value)| Divergence {
                surface,
                surface_id,
                severity,
                kind,
                a_value,
                b_value,
            },
        )
}

proptest! {
    /// A `Capture` (the offline fixture format) survives a JSON round trip
    /// byte-for-byte. The differential oracle diffs these files offline, so
    /// any drift introduced by serde itself would poison the comparison.
    #[test]
    fn capture_json_roundtrip_is_lossless(
        label in ".*",
        surfaces in prop::collection::vec(
            (
                ".*",
                arb_severity(),
                prop_oneof![
                    ".*".prop_map(Ok),
                    ".*".prop_map(Err),
                ],
            ),
            0..8,
        ),
    ) {
        let capture = Capture {
            label,
            surfaces: surfaces
                .into_iter()
                .map(|(name, severity, value)| CapturedSurface { name, severity, value })
                .collect(),
        };
        let json = serde_json::to_string(&capture).expect("serialize");
        let back: Capture = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(capture, back);
    }

    /// `worst()` always returns the maximum severity by rank, for any
    /// divergence list.
    #[test]
    fn worst_is_the_max_rank(divergences in prop::collection::vec(arb_divergence(), 0..8)) {
        let report = DifferentialReport {
            label_a: "a".into(),
            label_b: "b".into(),
            surfaces: 0,
            agreed: 0,
            diverged: 0,
            errors: 0,
            divergences: divergences.clone(),
        };
        let expected = divergences
            .iter()
            .map(|d| d.severity)
            .max_by_key(severity_rank);
        prop_assert_eq!(report.worst(), expected);
    }

    /// `engine_divergence_count` always agrees with the iterator and never
    /// counts persona-intended divergences.
    #[test]
    fn engine_divergence_count_matches_filter(
        divergences in prop::collection::vec(arb_divergence(), 0..8),
    ) {
        let report = DifferentialReport {
            label_a: "a".into(),
            label_b: "b".into(),
            surfaces: 0,
            agreed: 0,
            diverged: 0,
            errors: 0,
            divergences: divergences.clone(),
        };
        let expected = divergences
            .iter()
            .filter(|d| d.kind == DivergenceKind::EngineDivergence)
            .count();
        prop_assert_eq!(report.engine_divergence_count(), expected);
        prop_assert!(report.engine_divergences().all(|d| d.kind == DivergenceKind::EngineDivergence));
    }

    /// `is_identical` is false exactly when there is drift or an error.
    #[test]
    fn is_identical_iff_no_drift_no_errors(diverged in 0usize..4, errors in 0usize..4) {
        let report = DifferentialReport {
            label_a: "a".into(),
            label_b: "b".into(),
            surfaces: diverged + errors,
            agreed: 0,
            diverged,
            errors,
            divergences: Vec::new(),
        };
        prop_assert_eq!(report.is_identical(), diverged == 0 && errors == 0);
    }

    /// `class_label` is stable and payload-independent for any payload
    /// string: the differential oracle compares stochastic surfaces by this
    /// label, so a payload-dependent label would flag healthy noise as drift.
    #[test]
    fn class_label_ignores_payload(outcome in arb_outcome()) {
        let label = outcome.class_label();
        let expected = match &outcome {
            ProbeOutcome::Pass => "Pass",
            ProbeOutcome::Drift(_) => "Drift",
            ProbeOutcome::Critical(_) => "Critical",
            ProbeOutcome::ProbeError(_) => "ProbeError",
        };
        prop_assert_eq!(label, expected);
        prop_assert_eq!(outcome.is_pass(), matches!(outcome, ProbeOutcome::Pass));
        prop_assert_eq!(outcome.is_critical(), matches!(outcome, ProbeOutcome::Critical(_)));
    }

    /// Every outcome survives a JSON round trip with its class label intact.
    #[test]
    fn probe_outcome_json_roundtrip_preserves_class(outcome in arb_outcome()) {
        let json = serde_json::to_string(&outcome).expect("serialize");
        let back: ProbeOutcome = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(back.class_label(), outcome.class_label());
        prop_assert_eq!(back, outcome);
    }
}
