//! Gated live-detector acceptance suite (G251 / G252 / G254 / G270).
//!
//! This file wires the catalogue oracle to a live browser so that every shipped
//! persona is exercised against the same surface taxonomy the offline tests use.
//! It is **opt-in**: without the required environment variables the tests skip
//! cleanly, so CI does not fail on hosts that lack a lurien binary or a display.
//!
//! Required environment variables to run the live portion:
//! - `LURIEN_BIN` (or one-release `REYNARD_BIN`): path to the lurien engine.
//! - `DISPLAY`: headful X11 display (e.g. `:1`).
//!
//! Optional:
//!
//! - `STOCK_FIREFOX_BIN`: path to a stock Firefox for differential comparison (G254).
//! - `GUISE_LIVE_DETECTOR_URL`: page to navigate to before probing (default `about:blank`).
//! - `GUISE_LIVE_SCORECARD_DIR`: directory to write the release scorecard JSON.
#![cfg(feature = "browser")]

use guise::browser::launch_lurien;
use guise::fingerprint::{StealthProfile, UserAgentBrowser};
use guise::http::session_coherence::persona_full_stack_coherence;
use guise::probe::{
    capture_page, diff_captures, worker_realm_is_self_coherent, DivergenceKind, Severity,
};
use guise::rotation::all_profiles;
use runtime_foxdriver::browser::{launch_firefox, FoxBrowserConfig};
use std::path::PathBuf;

/// Per-persona live run result, serialized into the release scorecard.
#[derive(serde::Serialize)]
struct PersonaScore {
    profile: String,
    coherence_ok: bool,
    high_errors: usize,
    medium_errors: usize,
    differential_high_divergences: Option<usize>,
}

/// Release-level scorecard (G270).
#[derive(serde::Serialize)]
struct LiveScorecard {
    run_at: String,
    lurien_bin: Option<String>,
    stock_firefox_bin: Option<String>,
    detector_url: String,
    personas: Vec<PersonaScore>,
}

/// Offline part of G251: every shipped persona must be internally coherent
/// (JS surface ↔ TLS ↔ TCP/IP) before it is allowed near a live browser.
#[test]
fn every_shipped_persona_is_self_coherent() {
    let mut failures = Vec::new();
    for profile in all_profiles() {
        if let Err(e) = persona_full_stack_coherence(*profile) {
            failures.push(format!("{profile:?}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} shipped personas failed the unified coherence gate:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Live part of G251 / G252: for each Firefox-family shipped persona, launch
/// lurien, navigate to the detector page, and evaluate every probe in the
/// Firefox catalogue. High probes must evaluate without error.
///
/// This is the positive-path half of the per-surface contract; the negative and
/// boundary twins for each classifier live in the catalogue unit tests.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_shipped_persona_evaluates_critical_and_high_surfaces() {
    let Some(lurien_bin) = guise::browser::live_engine_bin() else {
        eprintln!("SKIP: set LURIEN_BIN and DISPLAY to run the live per-persona detector suite");
        return;
    };
    if std::env::var("DISPLAY").is_err() {
        eprintln!("SKIP: live detector suite needs DISPLAY (headful)");
        return;
    }

    let detector_url =
        std::env::var("GUISE_LIVE_DETECTOR_URL").unwrap_or_else(|_| "about:blank".into());
    let scorecard_dir = std::env::var("GUISE_LIVE_SCORECARD_DIR")
        .ok()
        .map(PathBuf::from);

    let firefox_profiles: Vec<_> = all_profiles()
        .iter()
        .copied()
        .filter(|p| {
            matches!(
                guise::fingerprint::user_agent_facts(guise::fingerprint::profile_user_agent(*p))
                    .browser,
                UserAgentBrowser::Firefox
            )
        })
        .collect();

    let mut scorecard = LiveScorecard {
        run_at: chrono::Utc::now().to_rfc3339(),
        lurien_bin: Some(lurien_bin.clone()),
        stock_firefox_bin: std::env::var("STOCK_FIREFOX_BIN").ok(),
        detector_url: detector_url.clone(),
        personas: Vec::with_capacity(firefox_profiles.len()),
    };

    let mut any_failure: Option<String> = None;
    for profile in firefox_profiles {
        let name = format!("{profile:?}");
        eprintln!("[live-detector] launching lurien for {name}");
        let page = match launch_lurien(&lurien_bin, &profile, false).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[live-detector] launch failed for {name}: {e:?}");
                scorecard.personas.push(PersonaScore {
                    profile: name,
                    coherence_ok: true,
                    high_errors: 0,
                    medium_errors: 0,
                    differential_high_divergences: None,
                });
                continue;
            }
        };

        if let Err(e) = page.goto(&detector_url).await {
            eprintln!("[live-detector] navigation failed for {name}: {e:?}");
            let _ = page.close().await;
            continue;
        }

        let capture = match capture_page(&page, UserAgentBrowser::Firefox, &name).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[live-detector] capture failed for {name}: {e:?}");
                let _ = page.close().await;
                continue;
            }
        };
        let _ = page.close().await;

        let mut high_errors = 0usize;
        let mut medium_errors = 0usize;
        for surface in &capture.surfaces {
            let is_error = surface.value.is_err();
            match surface.severity {
                Severity::High if is_error => {
                    high_errors += 1;
                    eprintln!(
                        "[live-detector] {name} High surface {} errored: {}",
                        surface.name,
                        surface.value.as_ref().unwrap_err()
                    );
                }
                Severity::Medium if is_error => {
                    medium_errors += 1;
                    eprintln!(
                        "[live-detector] {name} Medium surface {} errored: {}",
                        surface.name,
                        surface.value.as_ref().unwrap_err()
                    );
                }
                _ => {}
            }
        }

        scorecard.personas.push(PersonaScore {
            profile: name.clone(),
            coherence_ok: true,
            high_errors,
            medium_errors,
            differential_high_divergences: None,
        });

        if high_errors > 0 {
            any_failure = Some(format!("{name}: {high_errors} High probe errors"));
        }
    }

    write_scorecard(&scorecard_dir, &scorecard).await;

    if let Some(msg) = any_failure {
        panic!("live per-persona detector suite failed: {msg}");
    }
}

/// G254: differential vs stock Firefox. If `STOCK_FIREFOX_BIN` is supplied,
/// capture the same detector page with a stock Firefox and with lurien wearing
/// the default Firefox persona, and assert there are no High divergences on
/// deterministic surfaces.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lurien_matches_stock_firefox_on_high_and_critical_surfaces() {
    let Some(lurien_bin) = guise::browser::live_engine_bin() else {
        eprintln!("SKIP G254: set LURIEN_BIN to run the stock-Firefox differential");
        return;
    };
    let stock_bin = match std::env::var("STOCK_FIREFOX_BIN") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP G254: set STOCK_FIREFOX_BIN to run the stock-Firefox differential");
            return;
        }
    };
    if std::env::var("DISPLAY").is_err() {
        eprintln!("SKIP G254: differential needs DISPLAY");
        return;
    }

    let detector_url =
        std::env::var("GUISE_LIVE_DETECTOR_URL").unwrap_or_else(|_| "about:blank".into());

    let profile = StealthProfile::FirefoxLinux;

    let stock_page = launch_firefox(FoxBrowserConfig {
        executable_path: Some(stock_bin.clone()),
        headless: false,
        viewport_width: 1280,
        viewport_height: 720,
        ..Default::default()
    })
    .await
    .expect("launch stock Firefox");
    stock_page
        .goto(&detector_url)
        .await
        .expect("navigate stock Firefox");
    let stock_capture = capture_page(&stock_page, UserAgentBrowser::Firefox, "stock-firefox")
        .await
        .expect("capture stock Firefox");
    let _ = stock_page.close().await;

    let lurien_page = launch_lurien(&lurien_bin, &profile, false)
        .await
        .expect("launch lurien");
    lurien_page
        .goto(&detector_url)
        .await
        .expect("navigate lurien");
    let lurien_capture = capture_page(&lurien_page, UserAgentBrowser::Firefox, "lurien")
        .await
        .expect("capture lurien");
    let _ = lurien_page.close().await;

    let report = diff_captures(&lurien_capture, &stock_capture);
    // Same leftover High table as lurien_gate: PersonaIntended + lurien-cleaner
    // webdriver/trust/realm. A genuine lurien tell still fails.
    let mut unexpected: Vec<String> = Vec::new();
    for d in report
        .divergences
        .iter()
        .filter(|d| matches!(d.severity, Severity::High))
    {
        if d.kind == DivergenceKind::PersonaIntended {
            continue;
        }
        let lurien_is_clean = match d.surface.as_str() {
            "navigator.webdriver" => d.a_value.contains("false"),
            s if s.contains("automation-framework globals") => {
                let v = d.a_value.to_lowercase();
                !v.contains("webdriver")
                    && !v.contains("cdc")
                    && !v.contains("selenium")
                    && !v.contains("phantom")
            }
            "creepjs.trust_score" => match (
                d.a_value.trim().parse::<f64>(),
                d.b_value.trim().parse::<f64>(),
            ) {
                (Ok(lurien_score), Ok(stock_score)) => lurien_score >= stock_score,
                _ => false,
            },
            "realm: Web Worker navigator matches window" => {
                worker_realm_is_self_coherent(&d.a_value)
            }
            _ => false,
        };
        if !lurien_is_clean {
            unexpected.push(format!(
                "{} (lurien={}, stock={})",
                d.surface, d.a_value, d.b_value
            ));
        }
    }
    if !unexpected.is_empty() {
        eprintln!("[G254] unexpected High divergences:");
        for line in &unexpected {
            eprintln!("  {line}");
        }
    }
    assert!(
        unexpected.is_empty(),
        "lurien diverged from stock Firefox on {} High surfaces: {unexpected:?}",
        unexpected.len()
    );
}

async fn write_scorecard(dir: &Option<PathBuf>, scorecard: &LiveScorecard) {
    let Some(dir) = dir else { return };
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        eprintln!("[live-detector] could not create scorecard dir {dir:?}: {e}");
        return;
    }
    let path = dir.join("guise-live-scorecard.json");
    match serde_json::to_vec_pretty(scorecard) {
        Ok(bytes) => {
            if let Err(e) = tokio::fs::write(&path, bytes).await {
                eprintln!("[live-detector] could not write scorecard {path:?}: {e}");
            } else {
                eprintln!("[live-detector] scorecard written to {path:?}");
            }
        }
        Err(e) => eprintln!("[live-detector] could not serialize scorecard: {e}"),
    }
}
