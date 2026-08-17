//! Property-based tests for guise-pacing.
//!
//! These pin universal invariants that must hold for *any* policy input,
//! not just the hand-crafted cases in the unit and adversarial suites:
//! every delay the crate produces is bounded, zero inputs stay zero, and
//! backoff schedules never shrink as the attempt count grows.

use guise_pacing::{
    capped_exponential_backoff, capped_pow2_backoff_ms, jittered_backoff, parse_retry_after,
    BackoffKind, BackoffPolicy, BoundedNormalDelay, Jitter, RequestPacer, MAX_PACING_BACKOFF,
    MAX_RETRY_AFTER_OBEYED,
};
use proptest::prelude::*;
use std::time::{Duration, SystemTime};

proptest! {
    /// For every (base, attempt, max) triple the pow2 backoff is bounded by
    /// both the configured max and the hard ceiling, and is zero exactly when
    /// an input is zero. A sleep outside these bounds is an effective hang.
    #[test]
    fn pow2_backoff_always_bounded(
        base_ms in 0u64..=u64::MAX,
        attempt in 0u32..=u32::MAX,
        max_ms in 0u64..=u64::MAX,
    ) {
        let delay = capped_pow2_backoff_ms(base_ms, attempt, max_ms);
        let ceiling = Duration::from_millis(max_ms).min(MAX_PACING_BACKOFF);
        prop_assert!(delay <= ceiling, "{delay:?} exceeded {ceiling:?}");
        prop_assert_eq!(delay == Duration::ZERO, base_ms == 0 || max_ms == 0);
    }

    /// The pow2 schedule never shrinks as attempts increase: retry sleeps
    /// that get *shorter* would hammer a struggling service.
    #[test]
    fn pow2_backoff_is_monotonic_in_attempt(
        base_ms in 1u64..=1_000_000,
        attempt in 0u32..64,
        max_ms in 1u64..=3_600_000,
    ) {
        let first = capped_pow2_backoff_ms(base_ms, attempt, max_ms);
        let next = capped_pow2_backoff_ms(base_ms, attempt + 1, max_ms);
        prop_assert!(next >= first, "attempt {attempt}: {first:?} then {next:?}");
    }

    /// General exponential backoff honors its configured cap for any policy,
    /// including hostile multipliers (NaN, sub-unit, infinite).
    #[test]
    fn exponential_backoff_honors_cap(
        initial_ms in 0u64..=u64::MAX,
        multiplier in any::<f64>(),
        attempt in 0u32..=u32::MAX,
        max_ms in 0u64..=u64::MAX,
    ) {
        let delay = capped_exponential_backoff(initial_ms, multiplier, attempt, max_ms);
        prop_assert!(delay <= Duration::from_millis(max_ms));
        prop_assert_eq!(delay == Duration::ZERO, initial_ms == 0 || max_ms == 0);
    }

    /// Sub-unit and non-finite multipliers are hostile policy data: they must
    /// behave exactly like a multiplier of 1.0 (constant delay), never as a
    /// shrinking or NaN sleep.
    #[test]
    fn hostile_multiplier_is_treated_as_one(
        initial_ms in 1u64..=1_000_000,
        attempt in 0u32..32,
        max_ms in 1u64..=3_600_000,
        hostile in prop_oneof![
            Just(f64::NAN),
            Just(f64::NEG_INFINITY),
            Just(0.0),
            Just(0.5),
            Just(f64::MIN_POSITIVE),
        ],
    ) {
        let hostile_delay = capped_exponential_backoff(initial_ms, hostile, attempt, max_ms);
        let unit_delay = capped_exponential_backoff(initial_ms, 1.0, attempt, max_ms);
        prop_assert_eq!(hostile_delay, unit_delay);
    }

    /// `parse_retry_after` never panics on arbitrary input and every accepted
    /// wait obeys the 60-second obedience cap.
    #[test]
    fn retry_after_never_panics_and_stays_capped(value in ".*") {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        if let Some(wait) = parse_retry_after(&value, now) {
            prop_assert!(wait <= MAX_RETRY_AFTER_OBEYED, "{wait:?} past cap for {value:?}");
        }
    }

    /// Pure-digit tokens are RFC delta-seconds: they always parse and are
    /// clamped (not rejected) above the cap.
    #[test]
    fn retry_after_digit_tokens_always_parse(seconds in 0u64..=1_000_000) {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let wait = parse_retry_after(&seconds.to_string(), now);
        prop_assert_eq!(wait, Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER_OBEYED)));
    }

    /// Deterministic jitter is a pure function of (base, nonce) and stays
    /// inside the ±20% envelope, clamped to the hard ceiling.
    #[test]
    fn jittered_backoff_deterministic_and_bounded(
        base_ms in 0u64..=u64::MAX,
        nonce in any::<u32>(),
    ) {
        let base = Duration::from_millis(base_ms);
        let first = jittered_backoff(base, nonce);
        let second = jittered_backoff(base, nonce);
        prop_assert_eq!(first, second, "jitter must be reproducible for a trace");
        prop_assert!(first <= MAX_PACING_BACKOFF);
        if base_ms == 0 {
            prop_assert_eq!(first, Duration::ZERO);
        } else if base <= MAX_PACING_BACKOFF {
            let nanos = base.as_nanos();
            prop_assert!(first.as_nanos() * 5 >= nanos * 4, "{first:?} below 80% of {base:?}");
            prop_assert!(first.as_nanos() <= nanos * 6 / 5 + 1, "{first:?} above 120% of {base:?}");
        }
    }

    /// `Jitter::range` accepts exactly the ordered pairs, and every sample
    /// lies in `[min, max)` (or equals the fixed bound when they coincide).
    #[test]
    fn jitter_envelope_bounds_hold(min_ms in 0u64..=100_000, max_ms in 0u64..=100_000) {
        let result = Jitter::range(min_ms, max_ms);
        prop_assert_eq!(result.is_ok(), min_ms <= max_ms);
        if let Ok(jitter) = result {
            let sample = jitter.sample_thread();
            if min_ms == max_ms {
                prop_assert_eq!(sample, Duration::from_millis(min_ms));
            } else {
                prop_assert!(sample >= Duration::from_millis(min_ms));
                prop_assert!(sample < Duration::from_millis(max_ms));
            }
        }
    }

    /// Unordered bounds are normalized and every bounded-normal sample stays
    /// inside the inclusive envelope, even for hostile extreme bounds.
    #[test]
    fn bounded_normal_samples_stay_inside(first in 0u64..=u64::MAX, second in 0u64..=u64::MAX) {
        let delay = BoundedNormalDelay::from_unordered_bounds(first, second);
        let (lo, hi) = (delay.min_ms(), delay.max_ms());
        prop_assert!(lo <= hi);
        let sample = delay.sample_thread();
        prop_assert!(sample >= Duration::from_millis(lo) && sample <= Duration::from_millis(hi),
            "{sample:?} outside [{lo}, {hi}]");
    }

    /// The challenge multiplier is confined to `1..=MAX_CHALLENGE_MULTIPLIER`
    /// for an arbitrary interleaving of successes and rate-limit signals.
    #[test]
    fn pacer_multiplier_stays_in_legal_band(signals in prop::collection::vec(any::<bool>(), 0..64)) {
        let mut pacer = RequestPacer::api_call();
        for rate_limited in signals {
            if rate_limited {
                pacer.record_rate_limit();
            } else {
                pacer.record_success();
            }
            let multiplier = pacer.challenge_multiplier();
            prop_assert!((1..=RequestPacer::MAX_CHALLENGE_MULTIPLIER).contains(&multiplier));
        }
        // And the emitted delay can never exceed the hard ceiling.
        let delay = pacer.next_delay_thread();
        prop_assert!(delay <= MAX_PACING_BACKOFF);
    }

    /// `BackoffPolicy::delay` honors the hard ceiling for every kind and any
    /// attempt counter, including the saturating extremes.
    #[test]
    fn policy_delay_never_exceeds_hard_ceiling(
        base_ms in 0u64..=u64::MAX,
        attempt in any::<u32>(),
        rate_limited in any::<bool>(),
    ) {
        let policy = BackoffPolicy::new(8, base_ms, base_ms);
        let kind = if rate_limited { BackoffKind::RateLimited } else { BackoffKind::Timeout };
        prop_assert!(policy.delay(kind, attempt) <= MAX_PACING_BACKOFF);
    }
}
