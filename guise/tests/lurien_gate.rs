//! The **lurien gate**: proves the patched engine binary is fingerprint-
//! identical to a stock Firefox across the surface catalogue.
//!
//! This is the payoff of the whole engine-fork effort: where the JS disguise
//! leaves residual tells (an overridden UA version, a sealed-getter `toString`,
//! a forced `hardwareConcurrency`), a correctly-patched lurien build should
//! diverge from stock on **nothing** the catalogue weights, because the spoof
//! lives in Gecko C++, not in JS a page can probe.
//!
//! `launch_lurien` drives the binary over BiDi with the persona injected via
//! `LURIEN_CONFIG`; `diff_pages` then diffs it against stock Firefox.
//!
//! Opt-in (needs a built lurien engine + a display):
//! ```text
//! LURIEN_BIN="$HOME/.local/share/lurien/lurien" \
//! STEALTH_FIREFOX=/usr/local/bin/firefox DISPLAY=:10 \
//!   cargo test -p guise --features browser --test lurien_gate -- --nocapture
//! ```
#![cfg(feature = "browser")]

use guise::browser::launch_lurien;
use guise::fingerprint::StealthProfile;
use guise::probe::{
    diff_pages, render_differential, worker_realm_is_self_coherent, DivergenceKind, Severity,
    UserAgentBrowser,
};
use runtime_foxdriver::{launch_firefox_self_managed, FoxBrowserConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lurien_is_fingerprint_identical_to_stock() {
    let Some(engine_bin) = guise::browser::live_engine_bin() else {
        eprintln!("SKIP lurien_gate: set LURIEN_BIN=/path/to/lurien to run");
        return;
    };
    if std::env::var("DISPLAY").is_err() {
        eprintln!("SKIP lurien_gate: no DISPLAY (headful needs an X server, e.g. DISPLAY=:10)");
        return;
    }

    // lurien: engine-level spoof via LURIEN_CONFIG, driven over BiDi.
    let lurien = launch_lurien(&engine_bin, &StealthProfile::FirefoxLinux, false)
        .await
        .expect("launch lurien binary");
    lurien.goto("about:blank").await.expect("nav lurien");

    // stock Firefox reference, driven as the SAME persona.
    // lurien claims the FirefoxLinux persona's UA (Firefox/150, matching the
    // measured FF-150 TLS impersonate profile we ship) via LURIEN_CONFIG at the
    // engine layer. The stock binary on this box is a DIFFERENT build (e.g.
    // Firefox/151), so comparing raw would flag every UA-derived surface
    // (userAgent + the codec/error/capability objects that embed it) purely on
    // the version digit (two different Firefox *builds*, not an engine tell).
    //
    // To isolate genuine engine fidelity, drive stock as the same persona UA via
    // `general.useragent.override` (the pref path; stock is not camoufox so it
    // can't take CAMOU_CONFIG). Both sides then report the persona UA, and the
    // diff reflects ONLY whether lurien's C++-level spoof matches a real
    // Firefox's native behaviour. Launched through the SAME self-managed
    // readiness-poll path so neither races rustenium's fixed post-spawn sleep.
    let Ok(stock_bin) = std::env::var("STEALTH_FIREFOX") else {
        eprintln!("SKIP lurien_gate: set STEALTH_FIREFOX=/path/to/stock/firefox for the reference");
        let _ = lurien.close().await;
        return;
    };
    // lurien claims the ENGINE's true Firefox version on the browser path (its
    // TLS + JS features are the real engine's), so drive the stock reference as
    // the same engine-aligned UA, otherwise the differential would flag the
    // version digit, which is an intentional lurien choice, not an engine tell.
    let persona_ua = {
        let ua = guise::profile_to_overrides(&StealthProfile::FirefoxLinux).user_agent;
        match guise::browser::firefox_engine_major(&engine_bin) {
            Some(m) => guise::browser::align_ua_to_engine(&ua, m),
            None => ua,
        }
    };
    let scfg = FoxBrowserConfig {
        headless: false,
        viewport_width: 1280,
        viewport_height: 720,
        executable_path: Some(stock_bin),
        user_js_content: Some(format!(
            "user_pref(\"general.useragent.override\", \"{}\");",
            persona_ua.replace('"', "\\\"")
        )),
        ..Default::default()
    };
    let stock = launch_firefox_self_managed(scfg)
        .await
        .expect("launch stock firefox");
    stock.goto("about:blank").await.expect("nav stock");

    let report = diff_pages(
        &lurien,
        &stock,
        UserAgentBrowser::Firefox,
        "lurien",
        "stock-firefox",
    )
    .await
    .expect("diff lurien vs stock");
    eprintln!("\n{}", render_differential(&report));
    let _ = lurien.close().await;
    let _ = stock.close().await;

    // Gate: zero High-severity ENGINE divergence (the surfaces a fingerprinter weights).
    // A divergence is excused only when it is NOT an engine tell:
    //
    //   1. PersonaIntended, the persona deliberately differs from the raw host
    //      (timezone, hardwareConcurrency, screen, UA/platform, …). The differential
    //      classifies these on the shared surface taxonomy (`divergence_kind_for_probe`),
    //      so the gate measures ENGINE fidelity, not persona application. (A persona
    //      surface's own correctness is verified by the per-surface probes + the live
    //      CreepJS/sannysoft detectors, not by this raw lurien-vs-host value compare.)
    //   2. webdriver / automation globals, the BiDi-driven stock reference is forced to
    //      `navigator.webdriver=true`; lurien's engine reports the real-user value.
    //      Accepted ONLY when lurien is the clean side, so a genuine lurien tell fails.
    //   3. creepjs.trust_score, an AGGREGATE human-likeness score, not a surface;
    //      lurien scoring ≥ stock is lurien being at least as trusted (never a tell).
    //   4. the worker-realm probe, its cross-browser VALUE differs whenever the persona
    //      differs from the host; the engine claim it verifies is INTRA-browser realm
    //      coherence, excused only when lurien's worker is self-coherent with its window.
    //
    // Every non-PersonaIntended exception is fail-closed (lurien must PROVE it is the
    // clean/coherent side) so a real engine tell still fails. (Medium/Low timing surfaces
    // are non-deterministic between launches and are not gated here.)
    let mut unexpected: Vec<String> = Vec::new();
    for d in report
        .divergences
        .iter()
        .filter(|d| d.severity == Severity::High)
    {
        if d.kind == DivergenceKind::PersonaIntended {
            continue;
        }
        // `diff_pages(lurien, stock, …)` → a_value = lurien, b_value = stock.
        let lurien_is_clean = match d.surface.as_str() {
            "navigator.webdriver" => d.a_value.contains("false"),
            s if s.contains("automation-framework globals") => {
                let v = d.a_value.to_lowercase();
                !v.contains("webdriver")
                    && !v.contains("cdc")
                    && !v.contains("selenium")
                    && !v.contains("phantom")
            }
            "creepjs.trust_score" => {
                match (
                    d.a_value.trim().parse::<f64>(),
                    d.b_value.trim().parse::<f64>(),
                ) {
                    (Ok(lurien_score), Ok(stock_score)) => lurien_score >= stock_score,
                    _ => false,
                }
            }
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
    assert!(
        unexpected.is_empty(),
        "lurien diverges from stock on High-severity surfaces, engine-level tells remain: {unexpected:?}\n\
         (each is a LURIEN_CONFIG key to set or a patch to fix)"
    );
    eprintln!(
        "[lurien gate] PASS: {} (zero High-severity divergence from stock)",
        report.summary()
    );
}
