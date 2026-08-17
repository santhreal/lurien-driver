//! Gap tests: documented limitations and deliberate behaviors of guise-pacing
//! that must stay pinned so any future change is a conscious decision, not a
//! silent drift. Each test names the behavior it pins and why it exists.

use guise_pacing::{Jitter, RequestPacer};
use std::time::Duration;

/// GAP: `Jitter::up_to(1)` can only ever produce a zero-millisecond delay,
/// because the upper bound is exclusive (`0..1` is a single-element domain).
/// A caller asking for "up to 1 ms of jitter" gets no jitter at all. This
/// pins the exclusive-bound contract; making the bound inclusive is a
/// deliberate behavioral change that must update this test.
#[test]
fn jitter_up_to_one_ms_is_always_zero() {
    let jitter = Jitter::up_to(1);
    for _ in 0..64 {
        assert_eq!(jitter.sample_thread(), Duration::ZERO);
    }
}

/// GAP: `record_http_status` treats only 429 and 403 as pushback and only
/// 2xx as recovery. Every other status - including 5xx server errors such as
/// 503 (a common rate-limit-adjacent signal) - leaves the multiplier
/// untouched. Pinning this so adding 5xx handling is a deliberate policy
/// change, not an accidental side effect of an edit elsewhere.
#[test]
fn record_http_status_ignores_5xx_and_other_unmapped_statuses() {
    for status in [500u16, 502, 503, 504, 404, 418, 101] {
        let mut pacer = RequestPacer::api_call();
        pacer.record_http_status(status);
        assert_eq!(
            pacer.challenge_multiplier(),
            1,
            "status {status} must not change the challenge multiplier"
        );
    }
}

/// GAP: `record_success` decays the multiplier by one step, it does not
/// reset it. A single success after a long challenge streak keeps the pacer
/// nearly as suspicious as before. This conservative decay is deliberate
/// (one lucky 200 does not prove the site stopped watching).
#[test]
fn record_success_decays_one_step_not_to_baseline() {
    let mut pacer = RequestPacer::api_call();
    for _ in 0..4 {
        pacer.record_rate_limit();
    }
    assert_eq!(pacer.challenge_multiplier(), 16);
    pacer.record_success();
    assert_eq!(pacer.challenge_multiplier(), 15);
}

/// GAP: `should_retry_after` and `BackoffPolicy::delay` are independent
/// gates: `delay` happily computes a clamped delay for attempts far beyond
/// the retry budget. Callers must check `should_retry_after` themselves;
/// `delay` does not return zero for out-of-budget attempts. Pinned so
/// merging the two is an explicit API decision.
#[test]
fn delay_does_not_enforce_retry_budget() {
    let policy = guise_pacing::BackoffPolicy::new(2, 500, 200);
    assert!(!policy.should_retry_after(1));
    // Attempt 5 is beyond budget, yet delay still answers.
    assert_eq!(
        policy.delay(guise_pacing::BackoffKind::RateLimited, 5),
        Duration::from_millis(16_000)
    );
}
