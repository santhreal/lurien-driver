//! Contract tests ensuring guise-oracle metadata, serialization, and doc alignment.

use guise_oracle::{
    Capture, CapturedSurface, DifferentialReport, Divergence, DivergenceKind, DriftReport,
    ProbeOutcome, ProbeReport, Severity,
};
use std::fs;

#[test]
fn cargo_toml_conforms_to_santh_standard() {
    let content = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");

    assert!(content.contains(r#"name = "guise-oracle""#));
    assert!(content.contains(r#"version = "0.1.3""#));
    assert!(content.contains(r#"rust-version = "1.85""#));
    assert!(content.contains(r#"[package.metadata.santh]"#));
    assert!(content.contains(r#"status = "beta""#));
    assert!(
        content.contains(r#"authors = ["Santh <64453045+santhreal@users.noreply.github.com>"]"#)
    );
}

#[test]
fn required_documentation_files_exist() {
    assert!(fs::metadata("README.md").is_ok(), "README.md missing");
    assert!(fs::metadata("SPEC.md").is_ok(), "SPEC.md missing");
    assert!(fs::metadata("CHANGELOG.md").is_ok(), "CHANGELOG.md missing");
}

#[test]
fn readme_contains_mandatory_sections() {
    let readme = fs::read_to_string("README.md").expect("read README.md");

    assert!(readme.contains("# guise-oracle"));
    assert!(readme.contains("https://img.shields.io/badge/santh-beta-blue"));
    assert!(readme.contains("## Quick Start"));
    assert!(readme.contains("## When to use / when not to use"));
    assert!(readme.contains("## Compared to alternatives"));
    assert!(readme.contains("## How it fits in Santh"));
    assert!(readme.contains("## License"));
}

#[test]
fn json_contract_shapes_are_stable() {
    let capture = Capture {
        label: "test-browser".into(),
        surfaces: vec![CapturedSurface {
            name: "nav.user_agent".into(),
            severity: Severity::High,
            value: Ok("Mozilla/5.0".into()),
        }],
    };

    let json = serde_json::to_string(&capture).expect("serialize capture");
    assert!(json.contains(r#""label":"test-browser""#));
    assert!(json.contains(r#""severity":"High""#));
    assert!(json.contains(r#""Ok":"Mozilla/5.0""#));

    let diff_report = DifferentialReport {
        label_a: "a".into(),
        label_b: "b".into(),
        surfaces: 1,
        agreed: 0,
        diverged: 1,
        errors: 0,
        divergences: vec![Divergence {
            surface: "nav.user_agent".into(),
            surface_id: Some("nav.user_agent".into()),
            severity: Severity::High,
            kind: DivergenceKind::EngineDivergence,
            a_value: "A".into(),
            b_value: "B".into(),
        }],
    };

    let diff_json = serde_json::to_string(&diff_report).expect("serialize diff report");
    assert!(diff_json.contains(r#""kind":"EngineDivergence""#));

    let drift_report = DriftReport {
        probed: 1,
        passed: 1,
        drift: 0,
        critical: 0,
        probe_errors: 0,
        per_probe: vec![ProbeReport {
            name: "p1".into(),
            severity: "High".into(),
            outcome: ProbeOutcome::Pass,
        }],
    };

    let drift_json = serde_json::to_string(&drift_report).expect("serialize drift report");
    assert!(drift_json.contains(r#""outcome":"Pass""#));
}
