//! Adversarial tests for guise-pacing: hostile policy values and boundary
//! inputs that must never panic, wrap, or produce an unbounded sleep.

use guise_pacing::{
    capped_exponential_backoff, parse_retry_after, BackoffKind, BackoffPolicy, BoundedNormalDelay,
    RequestPacer, MAX_PACING_BACKOFF, MAX_RETRY_AFTER_OBEYED,
};
use rand::SeedableRng;
use std::time::{Duration, SystemTime};

/// Reproducer for a real bug: `should_retry_after` computed `attempt + 1`,
/// which overflows at `u32::MAX` (panic in debug, wrap-to-zero in release,
/// where it would wrongly permit another retry). A hostile or corrupt retry
/// counter must saturate to "do not retry", never crash the scanner.
#[test]
fn should_retry_after_max_attempt_saturates_to_no() {
    let policy = BackoffPolicy::gossan_compatible();
    assert!(!policy.should_retry_after(u32::MAX));
    assert!(!policy.should_retry_after(u32::MAX - 1));
}

/// Boundary: `max_retries = 0` means never retry, and `max_retries = 1`
/// means the first failure is already the last attempt.
#[test]
fn should_retry_after_zero_and_one_retry_policies() {
    let never = BackoffPolicy::new(0, 100, 100);
    assert!(!never.should_retry_after(0));

    let once = BackoffPolicy::new(1, 100, 100);
    assert!(!once.should_retry_after(0));
}

/// Proving pair for the overflow fix: the ordinary schedule is unchanged.
/// With `max_retries = 4`, attempts 0, 1, 2 may retry and attempt 3 is last.
#[test]
fn should_retry_after_normal_schedule_is_unchanged() {
    let policy = BackoffPolicy::new(4, 100, 100);
    assert!(policy.should_retry_after(0));
    assert!(policy.should_retry_after(2));
    assert!(!policy.should_retry_after(3));
    assert!(!policy.should_retry_after(4));
}

/// Adversarial: a delay request at the extreme attempt count must still
/// clamp to the hard ceiling instead of overflowing shift arithmetic.
#[test]
fn delay_at_max_attempt_stays_clamped() {
    let policy = BackoffPolicy::new(8, 500, 200);
    let delay = policy.delay(BackoffKind::RateLimited, u32::MAX);
    assert_eq!(delay, Duration::from_secs(60));
}
/// Adversarial: Retry-After delta-seconds with 100 digits of '9' must saturate to
/// MAX_RETRY_AFTER_OBEYED (60s), not return None (which would silently drop the cooldown).
#[test]
fn retry_after_hostile_100_digit_delta_seconds_saturates_to_cap() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let hostile_header = "9".repeat(100);
    assert_eq!(
        parse_retry_after(&hostile_header, now),
        Some(MAX_RETRY_AFTER_OBEYED)
    );
}

/// Adversarial: `capped_exponential_backoff` with `max_ms = u64::MAX` must clamp
/// to `MAX_PACING_BACKOFF` (60s) rather than returning a 584-year sleep.
#[test]
fn capped_exponential_backoff_hostile_max_ms_clamps_to_ceiling() {
    assert_eq!(
        capped_exponential_backoff(1000, 2.0, 30, u64::MAX),
        MAX_PACING_BACKOFF
    );
}

/// Adversarial: `RequestPacer` constructed with a hostile `u64::MAX` envelope must
/// saturate to `MAX_PACING_BACKOFF` (60s) when sampled.
#[test]
fn sampling_request_pacer_with_hostile_u64_max_envelope_clamps_to_ceiling() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let pacer = RequestPacer::new(BoundedNormalDelay::from_unordered_bounds(
        u64::MAX,
        u64::MAX,
    ));
    assert_eq!(pacer.next_delay(&mut rng), MAX_PACING_BACKOFF);
}
