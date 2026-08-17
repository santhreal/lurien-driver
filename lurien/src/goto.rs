//! Navigate and classify. Captcha is a property of `goto`.

use crate::error::Error;
use runtime_foxdriver::Page;

/// Closed challenge kinds. Unknown is `fail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeKind {
    /// No challenge. Document is usable.
    None,
    /// Score-class (managed Cloudflare). Token wait.
    Score,
    /// Checkbox. v1.1.
    Checkbox,
    /// Visual grid. v1.1.
    Visual,
    /// Slider. v1.1.
    Slider,
    /// Audio. v1.1.
    Audio,
    /// Proof of work. v1.1.
    Pow,
    /// Typed refuse. Never CapSolver.
    Fail,
}

impl ChallengeKind {
    /// Parse a catalog kind string. Unknown → [`ChallengeKind::Fail`].
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "score" => Self::Score,
            "checkbox" => Self::Checkbox,
            "visual" => Self::Visual,
            "slider" => Self::Slider,
            "audio" => Self::Audio,
            "pow" => Self::Pow,
            _ => Self::Fail,
        }
    }

    /// v1 claims only `none` and `score`.
    #[must_use]
    pub const fn claimed_in_v1(self) -> bool {
        matches!(self, Self::None | Self::Score)
    }
}

/// Result of a `goto`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotoOutcome {
    /// Final URL after navigation.
    pub url: String,
    /// Classified kind.
    pub kind: ChallengeKind,
    /// What the engine did, when the engine acted. `None` means this page
    /// presented nothing the observer had to touch, or the engine in use has no
    /// challenge subsystem: either way the classification came from the page.
    pub engine: Option<crate::challenge::EngineOutcome>,
}

/// Navigate, then classify. No `auto_solve`. No third-party solver.
///
/// The engine observes every browsing context, including the cross-origin one
/// that paints the widget, and reports what it did. When it reports, it wins:
/// only that context can see whether the vendor wrote its token. When it stays
/// silent the page probe classifies, which is what happens on a clean page and
/// on an engine built without the subsystem.
pub async fn goto(page: &Page, url: &str) -> Result<GotoOutcome, Error> {
    page.goto(url)
        .await
        .map_err(|e| Error::Other(format!("goto {url}: {e}")))?;
    let evidence = crate::challenge::ChallengeConfig::for_process().evidence;
    let (kind, engine) = classify_with_score_wait(page, &evidence, url).await?;
    let final_url = page.url().await.unwrap_or_else(|_| url.to_string());
    if let Some(report) = engine.as_ref() {
        if !report.solved {
            let detail = report
                .error
                .clone()
                .unwrap_or_else(|| "the engine reported no vendor write".to_string());
            return match report.kind.as_str() {
                "score" | "none" => Err(Error::ScoreFailed { detail }),
                other => Err(Error::HardCaptcha {
                    kind: format!("{other}: {detail}"),
                }),
            };
        }
        return Ok(GotoOutcome {
            url: final_url,
            kind,
            engine,
        });
    }
    match kind {
        ChallengeKind::None | ChallengeKind::Score => Ok(GotoOutcome {
            url: final_url,
            kind,
            engine,
        }),
        ChallengeKind::Fail => Err(Error::ScoreFailed {
            detail: format!("catalog classified {url} as fail"),
        }),
        other => Err(Error::HardCaptcha {
            kind: format!("{other:?}").to_ascii_lowercase(),
        }),
    }
}

const SCORE_WAIT_MS: u64 = 8_000;
const WIDGET_SETTLE_MS: u64 = 2_000;
const SCORE_POLL_MS: u64 = 250;
/// How long a page that looks like a challenge waits for the engine to report
/// before the page probe decides on its own. The engine budget is longer than
/// this on purpose: a click plus a vendor round trip outlives a probe.
const ENGINE_WAIT_MS: u64 = 25_000;

/// Decide whether a classify probe is final.
///
/// A score-like widget without a token is `score-pending`, not checkbox.
/// `none` is held for [`WIDGET_SETTLE_MS`] so a late widget can appear.
/// `score-pending` is held for [`SCORE_WAIT_MS`] waiting for a token.
fn decide_kind(raw: &str, elapsed_ms: u64) -> Option<ChallengeKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "score" => Some(ChallengeKind::Score),
        "none" if elapsed_ms >= WIDGET_SETTLE_MS => Some(ChallengeKind::None),
        "none" => None,
        "score-pending" if elapsed_ms >= SCORE_WAIT_MS => Some(ChallengeKind::Fail),
        "score-pending" => None,
        other => Some(ChallengeKind::parse(other)),
    }
}

/// Classify, watching both the page and the engine.
///
/// Two observers with one rule: the engine wins when it speaks, because it is
/// the only one that can see inside the widget's own context. Until it does, the
/// page probe holds a score-like widget open rather than calling it solved.
async fn classify_with_score_wait(
    page: &Page,
    evidence: &std::path::Path,
    url: &str,
) -> Result<(ChallengeKind, Option<crate::challenge::EngineOutcome>), Error> {
    let start = tokio::time::Instant::now();
    let mut page_kind: Option<ChallengeKind> = None;
    loop {
        if let Some(report) = crate::challenge::outcome_for(evidence, url) {
            let kind = ChallengeKind::parse(&report.kind);
            return Ok((kind, Some(report)));
        }
        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        if page_kind.is_none() {
            let raw = classify_raw(page).await?;
            page_kind = decide_kind(&raw, elapsed_ms);
        }
        // A clean page needs no engine report, so it returns as soon as the probe
        // settles. A page the probe called a challenge waits for the engine,
        // which may still be clicking.
        if let Some(kind) = page_kind {
            // A page the engine took is not decided by the probe. The probe cannot
            // see into the widget's own context, so it calls a page being solved
            // clean, and returning that closes the session mid-solve.
            let engine_busy = crate::challenge::taken(evidence, url);
            let engine_could_still_report = (engine_busy
                || !matches!(kind, ChallengeKind::None))
                && elapsed_ms < ENGINE_WAIT_MS;
            if !engine_could_still_report {
                return Ok((kind, None));
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(SCORE_POLL_MS)).await;
    }
}

/// Probe the document with selectors compiled from `captcha/kinds/`. The vendor
/// names live in that directory; this function only knows kinds, so adding a
/// vendor changes the probe without changing this file.
///
/// A token already written, or a cleared-challenge cookie, is `score`. A widget
/// of a score-like kind is `score-pending`, which the caller holds until the
/// token arrives. Any other widget reports its own kind, and interactive kinds
/// fail closed upstream because v1 claims only `score`.
async fn classify_raw(page: &Page) -> Result<String, Error> {
    let js = classify_js();
    let eval = page
        .evaluate(&js)
        .await
        .map_err(|e| Error::Other(format!("classify: {e}")))?;
    Ok(eval
        .into_value::<String>()
        .unwrap_or_else(|_| "fail".into()))
}

/// Build the probe from the catalog. Deterministic, so a snapshot test can pin
/// it, and cheap enough to build per poll.
fn classify_js() -> String {
    use std::fmt::Write as _;
    let mut js = String::from("(function(){\n");
    let token = crate::catalog::token_selector();
    if !token.is_empty() {
        let _ = writeln!(
            js,
            "  var tok = document.querySelector({});\n  if (tok && tok.value && tok.value.length > 0) return \"score\";",
            json_string(&token)
        );
    }
    let cookies = crate::catalog::cleared_cookies();
    if !cookies.is_empty() {
        let names = cookies
            .iter()
            .map(|c| json_string(c))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            js,
            "  try {{ var cleared = [{names}];\n    if (document.cookie.split(';').some(function(c){{ return cleared.indexOf(c.trim().split('=')[0]) !== -1; }})) return \"score\";\n  }} catch (e) {{}}"
        );
    }
    for kind in crate::catalog::probe_kinds() {
        let selector = crate::catalog::widget_selector(kind);
        if selector.is_empty() {
            continue;
        }
        let reported = if crate::catalog::is_score_like(kind) {
            "score-pending"
        } else {
            kind
        };
        let _ = writeln!(
            js,
            "  if (document.querySelector({})) return {};",
            json_string(&selector),
            json_string(reported)
        );
    }
    js.push_str("  return \"none\";\n})()");
    js
}

/// Quote a value for embedding in the probe. The catalog is trusted data, but a
/// selector containing a quote would still break the script.
fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_kinds_parse() {
        assert_eq!(ChallengeKind::parse("none"), ChallengeKind::None);
        assert_eq!(ChallengeKind::parse("score"), ChallengeKind::Score);
        assert_eq!(ChallengeKind::parse("CHECKBOX"), ChallengeKind::Checkbox);
    }

    #[test]
    fn unknown_kind_fails_closed() {
        assert_eq!(ChallengeKind::parse("nonesuch"), ChallengeKind::Fail);
        assert_eq!(ChallengeKind::parse(""), ChallengeKind::Fail);
        assert!(!ChallengeKind::Checkbox.claimed_in_v1());
        assert!(ChallengeKind::Score.claimed_in_v1());
    }

    #[test]
    fn pending_score_waits_then_fails_closed() {
        assert_eq!(decide_kind("score", 0), Some(ChallengeKind::Score));
        assert_eq!(decide_kind("score-pending", 0), None);
        assert_eq!(decide_kind("score-pending", 7_999), None);
        assert_eq!(
            decide_kind("score-pending", 8_000),
            Some(ChallengeKind::Fail)
        );
        assert_eq!(decide_kind("none", 0), None);
        assert_eq!(decide_kind("none", 1_999), None);
        assert_eq!(decide_kind("none", 2_000), Some(ChallengeKind::None));
        assert_eq!(decide_kind("visual", 0), Some(ChallengeKind::Visual));
        assert_eq!(decide_kind("checkbox", 0), Some(ChallengeKind::Checkbox));
    }

    #[test]
    fn the_probe_is_built_from_the_catalog_and_orders_score_first() {
        let js = classify_js();
        // A token hook and a cleared-cookie check must both be present, or a
        // solved page would be reported as a fresh challenge.
        assert!(js.contains("tok.value"), "{js}");
        assert!(js.contains("document.cookie"), "{js}");
        let pending = js.find("score-pending").expect("score-pending branch");
        for kind in crate::catalog::probe_kinds() {
            if crate::catalog::is_score_like(kind) {
                continue;
            }
            let at = js
                .find(&format!("return \"{kind}\""))
                .unwrap_or_else(|| panic!("kind {kind} missing from probe: {js}"));
            assert!(at > pending, "{kind} is probed before the token wait");
        }
        assert!(js.trim_end().ends_with("})()"), "{js}");
    }

    #[test]
    fn every_catalogued_kind_can_be_parsed_back() {
        // A kind the probe can return but `ChallengeKind` cannot parse would be
        // silently downgraded to `fail`.
        for kind in crate::catalog::probe_kinds() {
            let reported = if crate::catalog::is_score_like(kind) {
                continue;
            } else {
                kind
            };
            assert_ne!(
                ChallengeKind::parse(reported),
                ChallengeKind::Fail,
                "probe can return {reported}, which parses as fail"
            );
        }
    }

    #[test]
    fn no_vendor_name_appears_in_this_module() {
        let source = include_str!("goto.rs");
        for binding in crate::catalog::CATALOG {
            assert!(
                !source.contains(binding.name),
                "goto.rs names vendor {}; the catalog owns vendor knowledge",
                binding.name
            );
        }
    }
}
