//! Adversarial and boundary tests for the guise-oracle report contracts.
//!
//! The `is_green` gate decides whether CI passes a stealth drift report, so
//! its percentage math must be exact: a floored integer threshold once let
//! 2-of-3 passing read as "green", and a deserialized hostile report with
//! counts near `usize::MAX` could overflow the multiplication outright.

use guise_oracle::{DriftReport, ProbeReport};

fn report(probed: usize, passed: usize, critical: usize) -> DriftReport {
    DriftReport {
        probed,
        passed,
        drift: 0,
        critical,
        probe_errors: 0,
        per_probe: Vec::<ProbeReport>::new(),
    }
}

/// Boundary: exactly 90% is green, one probe below is not. The pre-fix
/// `passed >= probed * 90 / 100` floored the threshold, so small reports
/// passed at far less than 90% (2/3 = 66% was "green").
#[test]
fn is_green_threshold_is_exact_ninety_percent() {
    assert!(report(10, 9, 0).is_green());
    assert!(!report(10, 8, 0).is_green());
    assert!(report(3, 3, 0).is_green());
    assert!(!report(3, 2, 0).is_green(), "2/3 passing is 66%, not green");
    assert!(report(1, 1, 0).is_green());
    assert!(!report(1, 0, 0).is_green());
}

/// Boundary: an empty report is never green (nothing was verified).
#[test]
fn is_green_rejects_empty_report() {
    assert!(!report(0, 0, 0).is_green());
}

/// Negative: any critical leak fails the gate no matter how clean the rest.
#[test]
fn is_green_rejects_any_critical() {
    assert!(!report(10, 10, 1).is_green());
}

/// Adversarial: counts near `usize::MAX` (reachable via hostile or corrupt
/// deserialized JSON) must not overflow the percentage multiplication.
/// The pre-fix form `probed * 90` panics in debug and wraps in release.
#[test]
fn is_green_survives_hostile_huge_counts() {
    let huge = report(usize::MAX, usize::MAX, 0);
    assert!(huge.is_green());
    let nearly = report(usize::MAX, usize::MAX / 2, 0);
    assert!(!nearly.is_green());
}

/// Adversarial: a report whose counts are internally inconsistent
/// (passed > probed, only possible via tampered JSON) must not panic and
/// must evaluate deterministically.
#[test]
fn is_green_tolerates_inconsistent_counts() {
    let tampered = report(5, 10, 0);
    // passed >= probed trivially satisfies >=90%; the point is no panic,
    // and the summary stays renderable for the incident log.
    let _ = tampered.is_green();
    assert!(tampered.summary().contains("10/5"));
}
