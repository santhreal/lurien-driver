//! Unit tests for guise-oracle primitives and helper methods.

use guise_oracle::{
    Capture, CapturedSurface, Determinism, DifferentialReport, Divergence, DivergenceKind,
    DriftReport, Probe, ProbeOutcome, ProbeReport, Severity, ThreeWayReport, ThreeWaySurface,
};

#[test]
fn severity_as_str_and_display() {
    assert_eq!(Severity::Low.as_str(), "Low");
    assert_eq!(Severity::Medium.as_str(), "Medium");
    assert_eq!(Severity::High.as_str(), "High");

    assert_eq!(format!("{}", Severity::Low), "Low");
    assert_eq!(format!("{}", Severity::Medium), "Medium");
    assert_eq!(format!("{}", Severity::High), "High");
}

#[test]
fn probe_outcome_predicates() {
    let pass = ProbeOutcome::Pass;
    let drift = ProbeOutcome::Drift("minor".into());
    let critical = ProbeOutcome::Critical("leak".into());
    let error = ProbeOutcome::ProbeError("js error".into());

    assert!(pass.is_pass());
    assert!(!pass.is_drift());
    assert!(!pass.is_critical());
    assert!(!pass.is_error());

    assert!(!drift.is_pass());
    assert!(drift.is_drift());
    assert!(!drift.is_critical());
    assert!(!drift.is_error());

    assert!(!critical.is_pass());
    assert!(!critical.is_drift());
    assert!(critical.is_critical());
    assert!(!critical.is_error());

    assert!(!error.is_pass());
    assert!(!error.is_drift());
    assert!(!error.is_critical());
    assert!(error.is_error());
}
#[test]
fn probe_outcome_message_extractor() {
    assert_eq!(ProbeOutcome::Pass.message(), None);
    assert_eq!(ProbeOutcome::Drift("minor".into()).message(), Some("minor"));
    assert_eq!(
        ProbeOutcome::Critical("leak".into()).message(),
        Some("leak")
    );
    assert_eq!(
        ProbeOutcome::ProbeError("js error".into()).message(),
        Some("js error")
    );
}

#[test]
fn probe_run_classifier() {
    let probe = Probe {
        name: "test.probe",
        js: "1 == 1",
        severity: Severity::Medium,
        classifier: |val| {
            if val.as_bool() == Some(true) {
                ProbeOutcome::Pass
            } else {
                ProbeOutcome::Critical("false expected".into())
            }
        },
        determinism: Determinism::Deterministic,
    };

    assert_eq!(
        probe.run_classifier(&serde_json::json!(true)),
        ProbeOutcome::Pass
    );
    assert_eq!(
        probe.run_classifier(&serde_json::json!(false)),
        ProbeOutcome::Critical("false expected".into())
    );
}

#[test]
fn differential_report_engine_worst_and_consistency() {
    let report = DifferentialReport {
        label_a: "a".into(),
        label_b: "b".into(),
        surfaces: 2,
        agreed: 0,
        diverged: 2,
        errors: 0,
        divergences: vec![
            Divergence {
                surface: "s1".into(),
                surface_id: None,
                severity: Severity::Low,
                kind: DivergenceKind::PersonaIntended,
                a_value: "1".into(),
                b_value: "2".into(),
            },
            Divergence {
                surface: "s2".into(),
                surface_id: None,
                severity: Severity::Medium,
                kind: DivergenceKind::EngineDivergence,
                a_value: "x".into(),
                b_value: "y".into(),
            },
        ],
    };

    assert!(report.is_consistent());
    assert_eq!(report.worst(), Some(Severity::Medium));
    assert_eq!(report.engine_worst(), Some(Severity::Medium));
    assert_eq!(report.engine_divergence_count(), 1);

    let inconsistent = DifferentialReport {
        surfaces: 10,
        agreed: 1,
        diverged: 1,
        errors: 0,
        ..report
    };
    assert!(!inconsistent.is_consistent());
}

#[test]
fn drift_report_consistency() {
    let report = DriftReport {
        probed: 3,
        passed: 2,
        drift: 1,
        critical: 0,
        probe_errors: 0,
        per_probe: vec![
            ProbeReport {
                name: "p1".into(),
                severity: "High".into(),
                outcome: ProbeOutcome::Pass,
            },
            ProbeReport {
                name: "p2".into(),
                severity: "Low".into(),
                outcome: ProbeOutcome::Pass,
            },
            ProbeReport {
                name: "p3".into(),
                severity: "Medium".into(),
                outcome: ProbeOutcome::Drift("minor".into()),
            },
        ],
    };

    assert!(report.is_consistent());

    let invalid_counts = DriftReport {
        probed: 5,
        passed: 2,
        ..report
    };
    assert!(!invalid_counts.is_consistent());
}

#[test]
fn capture_offline_diffing() {
    let cap_a = Capture {
        label: "stock".into(),
        surfaces: vec![
            CapturedSurface {
                name: "webgl.vendor".into(),
                severity: Severity::Medium,
                value: Ok("Mesa".into()),
            },
            CapturedSurface {
                name: "navigator.hardwareConcurrency".into(),
                severity: Severity::High,
                value: Ok("8".into()),
            },
            CapturedSurface {
                name: "audio.latency".into(),
                severity: Severity::Low,
                value: Ok("0.01".into()),
            },
        ],
    };

    let cap_b = Capture {
        label: "lurien".into(),
        surfaces: vec![
            CapturedSurface {
                name: "webgl.vendor".into(),
                severity: Severity::Medium,
                value: Ok("Apple".into()),
            },
            CapturedSurface {
                name: "navigator.hardwareConcurrency".into(),
                severity: Severity::High,
                value: Ok("8".into()),
            },
            CapturedSurface {
                name: "audio.latency".into(),
                severity: Severity::Low,
                value: Ok("0.02".into()),
            },
        ],
    };

    let report = cap_a.diff(&cap_b);

    assert_eq!(report.surfaces, 3);
    assert_eq!(report.agreed, 1);
    assert_eq!(report.diverged, 2);
    assert_eq!(report.errors, 0);
    assert!(report.is_consistent());

    // Divergences should be sorted High -> Medium -> Low
    assert_eq!(report.divergences[0].surface, "webgl.vendor");
    assert_eq!(report.divergences[0].severity, Severity::Medium);
    assert_eq!(report.divergences[1].surface, "audio.latency");
    assert_eq!(report.divergences[1].severity, Severity::Low);
}
#[test]
fn capture_diffing_symmetric_severity() {
    let cap_a = Capture {
        label: "browser_a".into(),
        surfaces: vec![CapturedSurface {
            name: "webgl.renderer".into(),
            severity: Severity::Low,
            value: Ok("Mesa".into()),
        }],
    };
    let cap_b = Capture {
        label: "browser_b".into(),
        surfaces: vec![CapturedSurface {
            name: "webgl.renderer".into(),
            severity: Severity::High,
            value: Ok("NVIDIA".into()),
        }],
    };

    let diff_ab = cap_a.diff(&cap_b);
    let diff_ba = cap_b.diff(&cap_a);

    assert_eq!(diff_ab.divergences[0].severity, Severity::High);
    assert_eq!(diff_ba.divergences[0].severity, Severity::High);
}

#[test]
fn three_way_report_agreed_count() {
    let report = ThreeWayReport {
        surfaces: 10,
        engine_wins: vec![ThreeWaySurface {
            surface: "s1".into(),
            surface_id: None,
            severity: Severity::High,
            stock_value: "a".into(),
            lurien_value: "a".into(),
            disguise_value: "b".into(),
        }],
        js_wins: vec![],
        everyone_loses: vec![],
    };

    assert_eq!(report.agreed_count(), 9);
}
#[test]
fn three_way_report_from_captures_and_consistency() {
    let stock = Capture {
        label: "stock".into(),
        surfaces: vec![
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::High,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s2".into(),
                severity: Severity::Medium,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s3".into(),
                severity: Severity::Low,
                value: Ok("1".into()),
            },
        ],
    };
    let lurien = Capture {
        label: "lurien".into(),
        surfaces: vec![
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::High,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s2".into(),
                severity: Severity::Medium,
                value: Ok("2".into()),
            },
            CapturedSurface {
                name: "s3".into(),
                severity: Severity::Low,
                value: Ok("2".into()),
            },
        ],
    };
    let disguise = Capture {
        label: "disguise".into(),
        surfaces: vec![
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::High,
                value: Ok("2".into()),
            },
            CapturedSurface {
                name: "s2".into(),
                severity: Severity::Medium,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s3".into(),
                severity: Severity::Low,
                value: Ok("3".into()),
            },
        ],
    };

    let report = ThreeWayReport::from_captures(&stock, &lurien, &disguise);
    assert!(report.is_consistent());
    assert_eq!(report.surfaces, 3);
    assert_eq!(report.agreed_count(), 0);
    assert_eq!(report.engine_wins.len(), 1); // s1: stock=1, lurien=1, disguise=2
    assert_eq!(report.js_wins.len(), 1); // s2: stock=1, lurien=2, disguise=1
    assert_eq!(report.everyone_loses.len(), 1); // s3: stock=1, lurien=2, disguise=3
}
#[test]
fn severity_from_str_and_from_str_trait() {
    use std::str::FromStr;

    assert_eq!(Severity::from_str("High"), Some(Severity::High));
    assert_eq!(Severity::from_str("medium"), Some(Severity::Medium));
    assert_eq!(Severity::from_str("LOW"), Some(Severity::Low));
    assert_eq!(Severity::from_str("invalid"), None);

    assert_eq!(<Severity as FromStr>::from_str("High"), Ok(Severity::High));
    assert!(<Severity as FromStr>::from_str("unknown").is_err());
}

#[test]
fn determinism_as_str_from_str_and_display() {
    use std::str::FromStr;

    assert_eq!(Determinism::Deterministic.as_str(), "Deterministic");
    assert_eq!(Determinism::Stochastic.as_str(), "Stochastic");

    assert_eq!(
        Determinism::from_str("deterministic"),
        Some(Determinism::Deterministic)
    );
    assert_eq!(
        Determinism::from_str("stochastic"),
        Some(Determinism::Stochastic)
    );
    assert_eq!(Determinism::from_str("invalid"), None);

    assert_eq!(format!("{}", Determinism::Deterministic), "Deterministic");
    assert_eq!(
        <Determinism as FromStr>::from_str("Stochastic"),
        Ok(Determinism::Stochastic)
    );
}

#[test]
fn divergence_kind_as_str_from_str_and_display() {
    use std::str::FromStr;

    assert_eq!(DivergenceKind::PersonaIntended.as_str(), "PersonaIntended");
    assert_eq!(
        DivergenceKind::EngineDivergence.as_str(),
        "EngineDivergence"
    );

    assert_eq!(
        DivergenceKind::from_str("persona_intended"),
        Some(DivergenceKind::PersonaIntended)
    );
    assert_eq!(
        DivergenceKind::from_str("engine-divergence"),
        Some(DivergenceKind::EngineDivergence)
    );
    assert_eq!(DivergenceKind::from_str("unknown"), None);

    assert_eq!(
        format!("{}", DivergenceKind::PersonaIntended),
        "PersonaIntended"
    );
    assert_eq!(
        <DivergenceKind as FromStr>::from_str("PersonaIntended"),
        Ok(DivergenceKind::PersonaIntended)
    );
}

#[test]
fn probe_outcome_display() {
    assert_eq!(format!("{}", ProbeOutcome::Pass), "Pass");
    assert_eq!(
        format!("{}", ProbeOutcome::Drift("minor".into())),
        "Drift: minor"
    );
    assert_eq!(
        format!("{}", ProbeOutcome::Critical("leak".into())),
        "Critical: leak"
    );
    assert_eq!(
        format!("{}", ProbeOutcome::ProbeError("fail".into())),
        "ProbeError: fail"
    );
}

#[test]
fn differential_report_persona_divergences() {
    let report = DifferentialReport {
        label_a: "a".into(),
        label_b: "b".into(),
        surfaces: 2,
        agreed: 0,
        diverged: 2,
        errors: 0,
        divergences: vec![
            Divergence {
                surface: "s1".into(),
                surface_id: None,
                severity: Severity::High,
                kind: DivergenceKind::EngineDivergence,
                a_value: "1".into(),
                b_value: "2".into(),
            },
            Divergence {
                surface: "s2".into(),
                surface_id: None,
                severity: Severity::Medium,
                kind: DivergenceKind::PersonaIntended,
                a_value: "1".into(),
                b_value: "3".into(),
            },
        ],
    };

    assert_eq!(report.engine_divergence_count(), 1);
    assert_eq!(report.persona_divergence_count(), 1);
    assert_eq!(report.persona_divergences().next().unwrap().surface, "s2");
}

#[test]
fn drift_report_per_probe_breakdown_consistency() {
    // Valid matching per_probe list
    let valid = DriftReport {
        probed: 2,
        passed: 1,
        drift: 0,
        critical: 1,
        probe_errors: 0,
        per_probe: vec![
            ProbeReport {
                name: "p1".into(),
                severity: "High".into(),
                outcome: ProbeOutcome::Pass,
            },
            ProbeReport {
                name: "p2".into(),
                severity: "High".into(),
                outcome: ProbeOutcome::Critical("leak".into()),
            },
        ],
    };
    assert!(valid.is_consistent());

    // Inconsistent per_probe breakdown (counts say passed=2, critical=0, but per_probe has critical)
    let invalid = DriftReport {
        probed: 2,
        passed: 2,
        drift: 0,
        critical: 0,
        probe_errors: 0,
        per_probe: vec![
            ProbeReport {
                name: "p1".into(),
                severity: "High".into(),
                outcome: ProbeOutcome::Pass,
            },
            ProbeReport {
                name: "p2".into(),
                severity: "High".into(),
                outcome: ProbeOutcome::Critical("leak".into()),
            },
        ],
    };
    assert!(!invalid.is_consistent());
}

#[test]
fn probe_report_severity_enum() {
    let pr = ProbeReport {
        name: "p1".into(),
        severity: "High".into(),
        outcome: ProbeOutcome::Pass,
    };
    assert_eq!(pr.severity_enum(), Some(Severity::High));

    let pr_bad = ProbeReport {
        name: "p2".into(),
        severity: "Unknown".into(),
        outcome: ProbeOutcome::Pass,
    };
    assert_eq!(pr_bad.severity_enum(), None);
}

#[test]
fn capture_consistency_and_duplicates() {
    let clean = Capture {
        label: "firefox".into(),
        surfaces: vec![
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::High,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s2".into(),
                severity: Severity::Low,
                value: Ok("2".into()),
            },
        ],
    };
    assert!(clean.is_consistent());
    assert!(!clean.has_duplicate_surfaces());

    let duplicate = Capture {
        label: "firefox".into(),
        surfaces: vec![
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::High,
                value: Ok("1".into()),
            },
            CapturedSurface {
                name: "s1".into(),
                severity: Severity::Low,
                value: Ok("2".into()),
            },
        ],
    };
    assert!(!duplicate.is_consistent());
    assert!(duplicate.has_duplicate_surfaces());

    let empty_label = Capture {
        label: "   ".into(),
        surfaces: vec![],
    };
    assert!(!empty_label.is_consistent());
}

#[test]
fn three_way_report_all_agree() {
    let report = ThreeWayReport {
        surfaces: 5,
        engine_wins: vec![],
        js_wins: vec![],
        everyone_loses: vec![],
    };
    assert!(report.all_agree());
    assert_eq!(report.agreed_count(), 5);

    let report_with_diff = ThreeWayReport {
        surfaces: 5,
        engine_wins: vec![ThreeWaySurface {
            surface: "s1".into(),
            surface_id: None,
            severity: Severity::High,
            stock_value: "1".into(),
            lurien_value: "1".into(),
            disguise_value: "2".into(),
        }],
        js_wins: vec![],
        everyone_loses: vec![],
    };
    assert!(!report_with_diff.all_agree());
    assert_eq!(report_with_diff.agreed_count(), 4);
}
