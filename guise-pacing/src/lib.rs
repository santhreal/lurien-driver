//! Cross-plane pacing primitives for requests and browser actions.
//!
//! Donor implementations used to carry local copies of jitter and exponential
//! backoff policy. This module keeps the timing math pure so HTTP clients,
//! browser drivers, and mobile executors can share one contract without taking
//! each other's async or transport dependencies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::panic
    )
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc
)]

use std::time::{Duration, SystemTime};

use rand::Rng;
use thiserror::Error;

/// Maximum number of retry attempts used by the fleet's HTTP backoff policy.
pub const BACKOFF_MAX_RETRIES: u32 = 4;

/// Base delay for 429 / rate-limit backoff.
pub const BACKOFF_429_BASE_MS: u64 = 500;

/// Base delay for transient timeout backoff.
pub const BACKOFF_TIMEOUT_BASE_MS: u64 = 200;

/// Upper bound for a server-named `Retry-After` cooldown.
///
/// Longer waits are treated as hostile or operationally unhelpful and are
/// capped so scanners can fail visibly instead of sleeping for minutes.
pub const MAX_RETRY_AFTER_OBEYED: Duration = Duration::from_secs(60);

/// Hard ceiling on any pacing/backoff delay produced by this crate.
///
/// Pathological base durations or attempt counts can saturate `u128`/`u64`
/// arithmetic to values like `u64::MAX` nanoseconds (~584 years). That is
/// operationally a hang, so every delay computation in this module clamps to
/// this bound. Callers that configure a smaller maximum keep their cap.
pub const MAX_PACING_BACKOFF: Duration = Duration::from_secs(60);

/// Errors returned by pacing policy constructors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PacingError {
    /// The configured exclusive upper bound is below the lower bound.
    #[error(
        "invalid jitter range: min_ms={min_ms} is greater than max_exclusive_ms={max_exclusive_ms}"
    )]
    InvalidJitterRange {
        /// Inclusive lower bound in milliseconds.
        min_ms: u64,
        /// Exclusive upper bound in milliseconds.
        max_exclusive_ms: u64,
    },
}
/// Convert `nanos` into a [`Duration`] without ever silently falling back to
/// `u64::MAX` nanoseconds (~584 years). The result is clamped to `max` and,
/// if `nanos` is representable, reconstructed exactly from its seconds and
/// subsecond parts.
fn duration_from_nanos_clamped(nanos: u128, max: Duration) -> Duration {
    let max_nanos = max.as_nanos();
    if nanos >= max_nanos {
        return max;
    }
    let secs = (nanos / 1_000_000_000) as u64;
    let subsec_nanos = (nanos % 1_000_000_000) as u64;
    Duration::from_secs(secs)
        .checked_add(Duration::from_nanos(subsec_nanos))
        .unwrap_or(max)
}

/// Random delay envelope with an exclusive upper bound.
///
/// The exclusive upper bound preserves the historical `0..max_ms` behaviour
/// from Karyx camouflage while making the policy reusable and deterministic
/// under a caller-supplied RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jitter {
    min_ms: u64,
    max_exclusive_ms: u64,
}

impl Jitter {
    /// No jitter.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            min_ms: 0,
            max_exclusive_ms: 0,
        }
    }

    /// A delay sampled from `0..max_exclusive_ms`.
    #[must_use]
    pub const fn up_to(max_exclusive_ms: u64) -> Self {
        Self {
            min_ms: 0,
            max_exclusive_ms,
        }
    }

    /// A delay sampled from `min_ms..max_exclusive_ms`.
    ///
    /// # Errors
    ///
    /// Returns [`PacingError::InvalidJitterRange`] when `min_ms` is greater
    /// than the exclusive upper bound.
    pub fn range(min_ms: u64, max_exclusive_ms: u64) -> Result<Self, PacingError> {
        if min_ms > max_exclusive_ms {
            return Err(PacingError::InvalidJitterRange {
                min_ms,
                max_exclusive_ms,
            });
        }
        Ok(Self {
            min_ms,
            max_exclusive_ms,
        })
    }

    /// Inclusive lower bound in milliseconds.
    #[must_use]
    pub const fn min_ms(self) -> u64 {
        self.min_ms
    }

    /// Exclusive upper bound in milliseconds.
    #[must_use]
    pub const fn max_exclusive_ms(self) -> u64 {
        self.max_exclusive_ms
    }

    /// Sample a delay from this envelope.
    ///
    /// When the upper bound is equal to the lower bound, this returns the
    /// fixed lower-bound delay.
    pub fn sample<R: Rng + ?Sized>(self, rng: &mut R) -> Duration {
        if self.max_exclusive_ms <= self.min_ms {
            return Duration::from_millis(self.min_ms);
        }
        Duration::from_millis(rng.gen_range(self.min_ms..self.max_exclusive_ms))
    }

    /// Sample using the process-local RNG from this crate's `rand` version.
    ///
    /// This avoids leaking `rand`'s trait version through downstream APIs.
    #[must_use]
    pub fn sample_thread(self) -> Duration {
        let mut rng = rand::thread_rng();
        self.sample(&mut rng)
    }
}

/// Bounded normally-distributed delay envelope for human-like pacing.
///
/// The distribution is centered between the bounds with a standard deviation
/// of one quarter of the range, then clamped to the configured bounds. This
/// matches the historical Karyx authenticated-scan pacing contract while
/// making the Box-Muller sampling reusable across Santh runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedNormalDelay {
    min_ms: u64,
    max_ms: u64,
}

impl BoundedNormalDelay {
    /// No delay.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            min_ms: 0,
            max_ms: 0,
        }
    }

    /// Build from explicit inclusive lower and upper bounds.
    ///
    /// Bounds are normalized so hostile or misordered config cannot panic.
    #[must_use]
    pub fn from_unordered_bounds(first_ms: u64, second_ms: u64) -> Self {
        Self {
            min_ms: first_ms.min(second_ms),
            max_ms: first_ms.max(second_ms),
        }
    }

    /// Inclusive lower bound in milliseconds.
    #[must_use]
    pub const fn min_ms(self) -> u64 {
        self.min_ms
    }

    /// Inclusive upper bound in milliseconds.
    #[must_use]
    pub const fn max_ms(self) -> u64 {
        self.max_ms
    }

    /// Sample a delay from the bounded normal distribution.
    pub fn sample<R: Rng + ?Sized>(self, rng: &mut R) -> Duration {
        if self.max_ms <= self.min_ms {
            return Duration::from_millis(self.min_ms);
        }

        let mean = (self.min_ms as f64 + self.max_ms as f64) / 2.0;
        let stddev = (self.max_ms - self.min_ms) as f64 / 4.0;
        let u1: f64 = rng.gen_range(f64::EPSILON..1.0);
        let u2: f64 = rng.gen_range(0.0..std::f64::consts::TAU);
        let z = (-2.0 * u1.ln()).sqrt() * u2.cos();
        let delay_ms = (mean + z * stddev).clamp(self.min_ms as f64, self.max_ms as f64);

        // Clamp again in the integer domain: above 2^53 the f64 rounding of
        // the bounds can land a few ulps outside the documented inclusive
        // envelope (a hostile `u64::MAX`-scale bound must still be honored).
        Duration::from_millis((delay_ms as u64).clamp(self.min_ms, self.max_ms))
    }

    /// Sample using the process-local RNG from this crate's `rand` version.
    ///
    /// This avoids leaking `rand`'s trait version through downstream APIs.
    #[must_use]
    pub fn sample_thread(self) -> Duration {
        let mut rng = rand::thread_rng();
        self.sample(&mut rng)
    }
}

/// Apply randomized symmetric percent jitter to a duration.
///
/// `percent` is interpreted around 100%, so `20` samples a multiplier in
/// `80..=120`. The calculation preserves sub-millisecond precision and
/// saturates on hostile inputs instead of panicking.
#[must_use]
pub fn percent_jitter(base: Duration, percent: u64) -> Duration {
    let mut rng = rand::thread_rng();
    percent_jitter_with_rng(base, percent, &mut rng)
}

/// Apply randomized symmetric percent jitter using the caller's RNG.
///
/// `percent` is interpreted around 100%, so `20` samples a multiplier in
/// `80..=120`. Percent values over 100 allow delays below the base down to
/// zero, and all arithmetic saturates instead of overflowing.
#[must_use]
pub fn percent_jitter_with_rng<R: Rng + ?Sized>(
    base: Duration,
    percent: u64,
    rng: &mut R,
) -> Duration {
    if base.is_zero() {
        return Duration::ZERO;
    }

    let lower = 100_u64.saturating_sub(percent);
    let upper = 100_u64.saturating_add(percent);
    let factor = if lower == upper {
        lower
    } else {
        rng.gen_range(lower..=upper)
    };

    let jittered_nanos = base.as_nanos().saturating_mul(u128::from(factor)) / 100;
    duration_from_nanos_clamped(jittered_nanos, MAX_PACING_BACKOFF)
}

/// Retry category whose base delay differs by failure mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffKind {
    /// Remote service returned a rate-limit response such as HTTP 429.
    RateLimited,
    /// Transport timed out before a response completed.
    Timeout,
}

/// Exponential retry backoff policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    max_retries: u32,
    rate_limited_base_ms: u64,
    timeout_base_ms: u64,
}

impl BackoffPolicy {
    /// Build a policy from explicit retry count and base delays.
    #[must_use]
    pub const fn new(max_retries: u32, rate_limited_base_ms: u64, timeout_base_ms: u64) -> Self {
        Self {
            max_retries,
            rate_limited_base_ms,
            timeout_base_ms,
        }
    }

    /// Policy matching the fleet's historical Gossan/Karyx retry contract.
    #[must_use]
    pub const fn gossan_compatible() -> Self {
        Self::new(
            BACKOFF_MAX_RETRIES,
            BACKOFF_429_BASE_MS,
            BACKOFF_TIMEOUT_BASE_MS,
        )
    }

    /// Maximum number of attempts before a caller should give up.
    #[must_use]
    pub const fn max_retries(self) -> u32 {
        self.max_retries
    }
    /// Base delay in milliseconds for HTTP 429 / rate-limited responses.
    #[must_use]
    pub const fn rate_limited_base_ms(self) -> u64 {
        self.rate_limited_base_ms
    }

    /// Base delay in milliseconds for transient timeouts.
    #[must_use]
    pub const fn timeout_base_ms(self) -> u64 {
        self.timeout_base_ms
    }

    /// Whether another retry should be attempted after `attempt`.
    ///
    /// Saturates: a corrupt `u32::MAX` attempt counter means "do not retry",
    /// never an overflow panic (debug) or wrap-to-zero that re-arms the retry
    /// loop (release).
    #[must_use]
    pub const fn should_retry_after(self, attempt: u32) -> bool {
        attempt.saturating_add(1) < self.max_retries
    }

    /// Delay for `attempt`, using saturating exponential growth.
    ///
    /// The result is clamped to [`MAX_PACING_BACKOFF`] so a pathological base
    /// delay or attempt count cannot turn a retry sleep into an effective hang.
    #[must_use]
    pub fn delay(self, kind: BackoffKind, attempt: u32) -> Duration {
        let base_ms = match kind {
            BackoffKind::RateLimited => self.rate_limited_base_ms,
            BackoffKind::Timeout => self.timeout_base_ms,
        };
        Duration::from_millis(saturating_pow2_ms(base_ms, attempt)).min(MAX_PACING_BACKOFF)
    }
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self::gossan_compatible()
    }
}

/// Calculate a capped exponential backoff delay from caller-owned policy data.
///
/// `attempt` is zero-based: attempt `0` returns `initial_ms`, attempt `1`
/// returns `initial_ms * multiplier`, and so on. Non-finite or sub-unit
/// multipliers are treated as `1.0` so bad policy data cannot create negative,
/// NaN, or shrinking retry sleeps.
#[must_use]
pub fn capped_exponential_backoff(
    initial_ms: u64,
    multiplier: f64,
    attempt: u32,
    max_ms: u64,
) -> Duration {
    if initial_ms == 0 || max_ms == 0 {
        return Duration::ZERO;
    }

    let effective_max_ms = max_ms.min(MAX_PACING_BACKOFF.as_millis() as u64);
    let bounded_multiplier = if multiplier.is_finite() && multiplier >= 1.0 {
        multiplier
    } else {
        1.0
    };
    let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
    let delay_ms =
        (initial_ms as f64 * bounded_multiplier.powi(exponent)).min(effective_max_ms as f64);

    if delay_ms.is_finite() {
        // Clamp again in the integer domain: above 2^53 the f64 rounding of
        // `initial_ms`/`effective_max_ms` can land a few ulps past the cap, which would
        // make the documented `max_ms` bound leak.
        Duration::from_millis((delay_ms as u64).min(effective_max_ms))
    } else {
        Duration::from_millis(effective_max_ms)
    }
}

/// Calculate a capped power-of-two exponential backoff for duration policies.
///
/// `attempt` is zero-based: attempt `0` returns `base`, attempt `1` returns
/// `base * 2`, and so on. The base is capped before multiplication and the
/// final delay is capped again, matching scanner retry contracts that treat
/// the maximum as both an input and output safety boundary.
#[must_use]
pub fn capped_pow2_backoff(base: Duration, attempt: u32, max: Duration) -> Duration {
    if base.is_zero() || max.is_zero() {
        return Duration::ZERO;
    }

    let effective_max = max.min(MAX_PACING_BACKOFF);
    let capped_base_nanos = base.as_nanos().min(effective_max.as_nanos());
    let multiplier = if attempt >= u128::BITS {
        u128::MAX
    } else {
        1_u128 << attempt
    };
    let capped_nanos = capped_base_nanos
        .saturating_mul(multiplier)
        .min(effective_max.as_nanos());

    duration_from_nanos_clamped(capped_nanos, effective_max)
}

/// Calculate a capped power-of-two exponential backoff from millisecond inputs.
///
/// This is a convenience wrapper around [`capped_pow2_backoff`] for HTTP and
/// scanner configuration surfaces that store retry policy as milliseconds.
#[must_use]
pub fn capped_pow2_backoff_ms(base_ms: u64, attempt: u32, max_ms: u64) -> Duration {
    capped_pow2_backoff(
        Duration::from_millis(base_ms),
        attempt,
        Duration::from_millis(max_ms),
    )
}

/// Parse a `Retry-After` header value at a caller-supplied clock reference.
///
/// Accepted forms are RFC delta-seconds and HTTP-date values. Empty values,
/// negative values, fractional seconds, malformed integers, and past dates
/// return `None`; valid waits are capped at [`MAX_RETRY_AFTER_OBEYED`].
#[must_use]
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        // RFC 9110 delta-seconds is 1*DIGIT: no sign, no fraction, no
        // whitespace inside the token. A signed or fractional value falls
        // through to the HTTP-date arm and fails there. Digit strings larger
        // than u64::MAX saturate to u64::MAX so valid long cooldowns clamp
        // to MAX_RETRY_AFTER_OBEYED instead of silently falling back to None.
        let seconds: u64 = trimmed.parse().unwrap_or(u64::MAX);
        let delay = Duration::from_secs(seconds);
        return Some(delay.min(MAX_RETRY_AFTER_OBEYED));
    }

    let target = httpdate::parse_http_date(trimmed).ok()?;
    let delay = target.duration_since(now).ok()?;
    Some(delay.min(MAX_RETRY_AFTER_OBEYED))
}

/// Apply deterministic ±20% jitter to a backoff duration.
///
/// The jitter is keyed by a caller-provided monotonic nonce so scan traces stay
/// reproducible while avoiding synchronized retries across concurrent clients.
#[must_use]
pub fn jittered_backoff(base: Duration, nonce: u32) -> Duration {
    if base.is_zero() {
        return Duration::ZERO;
    }

    let mut x = nonce ^ 0x9E37_79B9;
    x ^= x.wrapping_shl(13);
    x ^= x.wrapping_shr(17);
    x ^= x.wrapping_shl(5);

    let multiplier_per_mille = 800 + u64::from(x % 401);
    let base_nanos = base.as_nanos();
    let jittered_nanos = base_nanos.saturating_mul(u128::from(multiplier_per_mille)) / 1000;
    duration_from_nanos_clamped(jittered_nanos, MAX_PACING_BACKOFF)
}

/// Human-like inter-request pacing policy.
///
/// `RequestPacer` is the single source of truth for *request* pacing (HTTP
/// client delays, browser navigation gaps) and *behavioral* timing (pre-action
/// think time, hover dwell, idle pauses). It samples from a bounded normal
/// distribution and adjusts to rate-limit / challenge signals so the delay
/// grows like a human who slows down when a site pushes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestPacer {
    base: BoundedNormalDelay,
    challenge_multiplier: u32,
}

impl RequestPacer {
    /// Hard ceiling on the challenge backoff multiplier.
    pub const MAX_CHALLENGE_MULTIPLIER: u32 = 16;

    /// Build a pacer from an explicit base delay distribution.
    #[must_use]
    pub const fn new(base: BoundedNormalDelay) -> Self {
        Self {
            base,
            challenge_multiplier: 1,
        }
    }

    /// Pacing between page loads: 800–3 000 ms, matching a human who takes in
    /// the page before the next navigation (also `ActionDelay::after_page_load`).
    #[must_use]
    pub fn page_load() -> Self {
        Self::new(BoundedNormalDelay::from_unordered_bounds(800, 3_000))
    }

    /// Pacing between sub-resource requests (favicon, prefetch, images): 100–400 ms.
    #[must_use]
    pub fn sub_resource() -> Self {
        Self::new(BoundedNormalDelay::from_unordered_bounds(100, 400))
    }

    /// Pacing for API / fetch calls: 300–1 200 ms.
    #[must_use]
    pub fn api_call() -> Self {
        Self::new(BoundedNormalDelay::from_unordered_bounds(300, 1_200))
    }

    /// Current challenge multiplier (1 = no penalty).
    #[must_use]
    pub const fn challenge_multiplier(self) -> u32 {
        self.challenge_multiplier
    }
    /// Get the underlying base delay distribution.
    #[must_use]
    pub const fn base(self) -> BoundedNormalDelay {
        self.base
    }

    /// Record a successful response so the pacer can decay any challenge
    /// penalty back toward normal pacing.
    pub fn record_success(&mut self) {
        self.challenge_multiplier = self.challenge_multiplier.saturating_sub(1).max(1);
    }

    /// Record a rate-limit / challenge signal (429, 403, captcha challenge).
    pub fn record_rate_limit(&mut self) {
        self.challenge_multiplier =
            (self.challenge_multiplier.saturating_mul(2)).min(Self::MAX_CHALLENGE_MULTIPLIER);
    }

    /// Convenience: update state from an HTTP status code.
    pub fn record_http_status(&mut self, status: u16) {
        match status {
            429 => self.record_rate_limit(),
            403 => self.record_rate_limit(),
            _ if (200..300).contains(&status) => self.record_success(),
            _ => {}
        }
    }

    /// Sample the next delay from the current distribution and multiplier.
    ///
    /// The multiplication saturates and the result is clamped to
    /// [`MAX_PACING_BACKOFF`], so a hostile base envelope (for example a
    /// `u64::MAX` millisecond upper bound from misordered config) cannot panic
    /// the process or sleep it effectively forever.
    pub fn next_delay<R: Rng + ?Sized>(&self, rng: &mut R) -> Duration {
        let base = self.base.sample(rng);
        base.saturating_mul(self.challenge_multiplier)
            .min(MAX_PACING_BACKOFF)
    }

    /// Sample the next delay using the process-local RNG.
    #[must_use]
    pub fn next_delay_thread(self) -> Duration {
        let mut rng = rand::thread_rng();
        self.next_delay(&mut rng)
    }
}

impl Default for RequestPacer {
    fn default() -> Self {
        Self::page_load()
    }
}

fn saturating_pow2_ms(base_ms: u64, attempt: u32) -> u64 {
    let multiplier = if attempt >= u64::BITS {
        u64::MAX
    } else {
        1_u64 << attempt
    };
    base_ms.saturating_mul(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn jitter_up_to_preserves_exclusive_upper_bound() {
        let mut rng = StdRng::seed_from_u64(7);
        let jitter = Jitter::up_to(10);
        for _ in 0..100 {
            let delay = jitter.sample(&mut rng);
            assert!(delay < Duration::from_millis(10));
        }
    }

    #[test]
    fn jitter_equal_bounds_is_fixed_delay() {
        let mut rng = StdRng::seed_from_u64(1);
        let jitter = Jitter::range(25, 25).unwrap();
        assert_eq!(jitter.sample(&mut rng), Duration::from_millis(25));
    }

    #[test]
    fn percent_jitter_preserves_symmetric_bounds() {
        let mut rng = StdRng::seed_from_u64(21);
        let base = Duration::from_millis(500);
        let lo = Duration::from_millis(400);
        let hi = Duration::from_millis(600);
        let mut distinct = std::collections::HashSet::new();

        for _ in 0..10_000 {
            let delay = percent_jitter_with_rng(base, 20, &mut rng);
            assert!(delay >= lo && delay <= hi, "{delay:?}");
            distinct.insert(delay);
        }

        assert_eq!(
            distinct.len(),
            41,
            "20% millisecond jitter over a 500ms base should preserve the historical 80..=120 factor domain"
        );
    }

    #[test]
    fn percent_jitter_zero_percent_is_fixed_delay() {
        let mut rng = StdRng::seed_from_u64(23);
        assert_eq!(
            percent_jitter_with_rng(Duration::from_millis(750), 0, &mut rng),
            Duration::from_millis(750)
        );
    }

    #[test]
    fn percent_jitter_zero_base_is_zero() {
        let mut rng = StdRng::seed_from_u64(25);
        assert_eq!(
            percent_jitter_with_rng(Duration::ZERO, 20, &mut rng),
            Duration::ZERO
        );
    }

    #[test]
    fn percent_jitter_large_percent_does_not_underflow() {
        let mut rng = StdRng::seed_from_u64(27);
        for _ in 0..1000 {
            let delay = percent_jitter_with_rng(Duration::from_millis(10), 250, &mut rng);
            assert!(delay <= Duration::from_millis(35), "{delay:?}");
        }
    }

    #[test]
    fn bounded_normal_delay_preserves_bounds_and_distribution_shape() {
        let mut rng = StdRng::seed_from_u64(31);
        let policy = BoundedNormalDelay::from_unordered_bounds(400, 600);
        let mut delays = Vec::with_capacity(10_000);

        for _ in 0..10_000 {
            let delay = policy.sample(&mut rng);
            assert!(
                delay >= Duration::from_millis(400) && delay <= Duration::from_millis(600),
                "{delay:?}"
            );
            delays.push(delay.as_millis() as f64);
        }

        let actual_mean = delays.iter().sum::<f64>() / delays.len() as f64;
        let variance = delays
            .iter()
            .map(|delay| (delay - actual_mean).powi(2))
            .sum::<f64>()
            / delays.len() as f64;
        let actual_stddev = variance.sqrt();

        assert!(
            (actual_mean - 500.0).abs() < 8.0,
            "mean {actual_mean} should stay centered"
        );
        assert!(
            actual_stddev > 35.0 && actual_stddev < 60.0,
            "stddev {actual_stddev} should preserve bounded normal shape"
        );
    }

    #[test]
    fn bounded_normal_delay_normalizes_misordered_bounds() {
        let mut rng = StdRng::seed_from_u64(33);
        let policy = BoundedNormalDelay::from_unordered_bounds(1_000, 100);

        assert_eq!(policy.min_ms(), 100);
        assert_eq!(policy.max_ms(), 1_000);
        for _ in 0..100 {
            let delay = policy.sample(&mut rng);
            assert!(
                delay >= Duration::from_millis(100) && delay <= Duration::from_millis(1_000),
                "{delay:?}"
            );
        }
    }

    #[test]
    fn bounded_normal_delay_equal_bounds_is_fixed_delay() {
        let mut rng = StdRng::seed_from_u64(35);
        let policy = BoundedNormalDelay::from_unordered_bounds(250, 250);
        assert_eq!(policy.sample(&mut rng), Duration::from_millis(250));
    }

    #[test]
    fn invalid_jitter_range_is_rejected() {
        assert!(matches!(
            Jitter::range(50, 10),
            Err(PacingError::InvalidJitterRange {
                min_ms: 50,
                max_exclusive_ms: 10
            })
        ));
    }

    #[test]
    fn gossan_backoff_schedule_is_pinned() {
        let policy = BackoffPolicy::gossan_compatible();
        assert_eq!(
            policy.delay(BackoffKind::RateLimited, 0),
            Duration::from_millis(500)
        );
        assert_eq!(
            policy.delay(BackoffKind::RateLimited, 3),
            Duration::from_millis(4_000)
        );
        assert_eq!(
            policy.delay(BackoffKind::Timeout, 2),
            Duration::from_millis(800)
        );
        assert!(policy.should_retry_after(2));
        assert!(!policy.should_retry_after(3));
    }

    #[test]
    fn capped_exponential_backoff_is_zero_based_and_capped() {
        assert_eq!(
            capped_exponential_backoff(100, 2.0, 0, 2_000),
            Duration::from_millis(100)
        );
        assert_eq!(
            capped_exponential_backoff(100, 2.0, 3, 2_000),
            Duration::from_millis(800)
        );
        assert_eq!(
            capped_exponential_backoff(100, 2.0, 10, 2_000),
            Duration::from_millis(2_000)
        );
    }

    #[test]
    fn capped_exponential_backoff_rejects_hostile_policy_values() {
        assert_eq!(
            capped_exponential_backoff(100, f64::NAN, 4, 2_000),
            Duration::from_millis(100)
        );
        assert_eq!(
            capped_exponential_backoff(100, 0.5, 4, 2_000),
            Duration::from_millis(100)
        );
        assert_eq!(
            capped_exponential_backoff(100, f64::INFINITY, 4, 2_000),
            Duration::from_millis(100)
        );
        assert_eq!(capped_exponential_backoff(0, 2.0, 4, 2_000), Duration::ZERO);
    }

    #[test]
    fn capped_pow2_backoff_is_zero_based_and_capped() {
        assert_eq!(
            capped_pow2_backoff(Duration::from_millis(100), 0, Duration::from_secs(60)),
            Duration::from_millis(100)
        );
        assert_eq!(
            capped_pow2_backoff(Duration::from_millis(100), 3, Duration::from_secs(60)),
            Duration::from_millis(800)
        );
        assert_eq!(
            capped_pow2_backoff(Duration::from_millis(100), 20, Duration::from_secs(60)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn capped_pow2_backoff_handles_zero_and_huge_inputs() {
        assert_eq!(
            capped_pow2_backoff(Duration::ZERO, 8, Duration::from_secs(60)),
            Duration::ZERO
        );
        assert_eq!(
            capped_pow2_backoff(Duration::from_secs(1), 8, Duration::ZERO),
            Duration::ZERO
        );
        assert_eq!(
            capped_pow2_backoff(Duration::MAX, u32::MAX, Duration::from_secs(60)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn capped_pow2_backoff_ms_matches_duration_policy() {
        assert_eq!(
            capped_pow2_backoff_ms(250, 4, 60_000),
            capped_pow2_backoff(Duration::from_millis(250), 4, Duration::from_secs(60))
        );
    }

    #[test]
    fn retry_after_delta_seconds_are_trimmed_and_capped() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        assert_eq!(
            parse_retry_after("\t30\t", now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(parse_retry_after("3600", now), Some(MAX_RETRY_AFTER_OBEYED));
        assert_eq!(parse_retry_after("0", now), Some(Duration::ZERO));
    }

    #[test]
    fn retry_after_rejects_malformed_delta_seconds() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        assert_eq!(parse_retry_after("", now), None);
        assert_eq!(parse_retry_after("   ", now), None);
        assert_eq!(parse_retry_after("-1", now), None);
        assert_eq!(parse_retry_after("12.5", now), None);
        assert_eq!(parse_retry_after("1 2", now), None);
    }

    #[test]
    fn retry_after_http_date_uses_caller_clock_and_cap() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let future = httpdate::fmt_http_date(now + Duration::from_secs(30));
        let past = httpdate::fmt_http_date(now - Duration::from_secs(30));
        let far = httpdate::fmt_http_date(now + Duration::from_secs(3600));

        let wait = parse_retry_after(&future, now).expect("future date should parse");
        assert!(wait >= Duration::from_secs(29) && wait <= Duration::from_secs(30));
        assert_eq!(parse_retry_after(&past, now), None);
        assert_eq!(parse_retry_after(&far, now), Some(MAX_RETRY_AFTER_OBEYED));
    }

    #[test]
    fn retry_after_rejects_non_gmt_http_date() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:28:00 +0530", now),
            None
        );
    }

    #[test]
    fn deterministic_jitter_is_bounded_and_varied() {
        let base = Duration::from_millis(1000);
        let lo = Duration::from_millis(800);
        let hi = Duration::from_millis(1200);
        let mut distinct = std::collections::HashSet::new();

        for nonce in 0..10_000_u32 {
            let delay = jittered_backoff(base, nonce);
            assert!(delay >= lo && delay <= hi, "nonce {nonce}: {delay:?}");
            if nonce < 128 {
                distinct.insert(delay);
            }
        }

        assert!(
            distinct.len() >= 40,
            "jitter produced only {} distinct delays",
            distinct.len()
        );
    }

    #[test]
    fn deterministic_jitter_zero_base_is_zero() {
        for nonce in 0..1000_u32 {
            assert_eq!(jittered_backoff(Duration::ZERO, nonce), Duration::ZERO);
        }
    }

    #[test]
    fn request_pacer_delays_stay_within_profile_bounds() {
        let mut rng = StdRng::seed_from_u64(41);
        let pacer = RequestPacer::page_load();
        for _ in 0..100 {
            let delay = pacer.next_delay(&mut rng);
            assert!(
                delay >= Duration::from_millis(800) && delay <= Duration::from_millis(3_000),
                "{delay:?}"
            );
        }
    }

    #[test]
    fn request_pacer_default_is_not_a_fixed_sleep() {
        let mut rng = StdRng::seed_from_u64(43);
        let pacer = RequestPacer::default();
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..100 {
            distinct.insert(pacer.next_delay(&mut rng));
        }
        assert!(
            distinct.len() > 50,
            "default pacer produced only {} distinct delays, looks fixed",
            distinct.len()
        );
    }

    #[test]
    fn request_pacer_doubles_then_decays_on_status_feedback() {
        let mut pacer = RequestPacer::api_call();
        pacer.record_http_status(429);
        assert_eq!(pacer.challenge_multiplier(), 2);
        pacer.record_http_status(429);
        assert_eq!(pacer.challenge_multiplier(), 4);
        pacer.record_http_status(200);
        assert_eq!(pacer.challenge_multiplier(), 3);
        pacer.record_http_status(200);
        assert_eq!(pacer.challenge_multiplier(), 2);
        pacer.record_http_status(200);
        assert_eq!(pacer.challenge_multiplier(), 1);
        pacer.record_http_status(200);
        assert_eq!(pacer.challenge_multiplier(), 1);
    }

    #[test]
    fn request_pacer_challenge_multiplier_is_capped() {
        let mut pacer = RequestPacer::sub_resource();
        for _ in 0..10 {
            pacer.record_rate_limit();
        }
        assert_eq!(
            pacer.challenge_multiplier(),
            RequestPacer::MAX_CHALLENGE_MULTIPLIER
        );
    }

    #[test]
    fn request_pacer_challenge_multiplies_delay() {
        let mut rng = StdRng::seed_from_u64(47);
        let base = RequestPacer::api_call();
        let mut challenged = base;
        challenged.record_rate_limit();
        let base_delay = base.next_delay(&mut rng);
        let challenged_delay = challenged.next_delay(&mut rng);
        // Both are sampled from the same bounded normal envelope, but the
        // challenged pacer multiplies by 2. It must be larger and stay within
        // the doubled profile bounds (300–1 200 ms → 600–2 400 ms).
        assert!(
            challenged_delay > base_delay,
            "challenged delay should exceed base delay"
        );
        assert!(
            challenged_delay >= Duration::from_millis(600)
                && challenged_delay <= Duration::from_millis(2_400),
            "{challenged_delay:?} outside doubled API-call bounds"
        );
    }

    #[test]
    fn percent_jitter_overflow_clamps_to_max_pacing_backoff() {
        let mut rng = StdRng::seed_from_u64(7);
        let delay = percent_jitter_with_rng(Duration::MAX, 20, &mut rng);
        assert!(
            delay <= MAX_PACING_BACKOFF,
            "overflow percent jitter {delay:?} exceeded ceiling"
        );
    }

    #[test]
    fn capped_pow2_backoff_overflow_clamps_to_configured_max() {
        let max = Duration::from_secs(60);
        let delay = capped_pow2_backoff(Duration::MAX, u32::MAX, max);
        assert!(
            delay <= max,
            "overflow capped_pow2 {delay:?} exceeded configured max {max:?}"
        );
        assert!(
            delay <= MAX_PACING_BACKOFF,
            "overflow capped_pow2 {delay:?} exceeded hard ceiling"
        );
    }

    #[test]
    fn jittered_backoff_overflow_clamps_to_max_pacing_backoff() {
        let delay = jittered_backoff(Duration::MAX, 42);
        assert!(
            delay <= MAX_PACING_BACKOFF,
            "overflow jittered_backoff {delay:?} exceeded ceiling"
        );
    }

    #[test]
    fn duration_from_nanos_clamped_never_falls_back_to_u64_max() {
        // One nanosecond past the hard ceiling must return the ceiling, not
        // `Duration::from_nanos(u64::MAX)` (~584 years).
        let ceiling = Duration::from_secs(60);
        let delay = duration_from_nanos_clamped(u128::MAX, ceiling);
        assert_eq!(delay, ceiling);
    }

    /// Regression: `RequestPacer::next_delay` used a plain `Duration *
    /// u32` multiply. A hostile base envelope (an upper bound near
    /// `u64::MAX` milliseconds, reachable through
    /// `BoundedNormalDelay::from_unordered_bounds`) combined with a challenge
    /// multiplier overflowed the multiplication and panicked the process. The
    /// delay must saturate and clamp to `MAX_PACING_BACKOFF` instead, because
    /// a stealth pacer that crashes or sleeps for centuries fails the scan
    /// loudly in the wrong way.
    #[test]
    fn request_pacer_hostile_base_saturates_instead_of_panicking() {
        let mut rng = StdRng::seed_from_u64(53);
        let mut pacer = RequestPacer::new(BoundedNormalDelay::from_unordered_bounds(
            u64::MAX,
            u64::MAX,
        ));
        pacer.record_rate_limit();
        pacer.record_rate_limit();
        let delay = pacer.next_delay(&mut rng);
        assert_eq!(delay, MAX_PACING_BACKOFF);
    }

    /// The challenge multiplier must still scale ordinary delays after the
    /// saturation fix: a multiplier of 2 on the API-call envelope doubles the
    /// upper bound from 1 200 ms to 2 400 ms, well under the ceiling.
    #[test]
    fn request_pacer_ordinary_multiplication_is_preserved() {
        let mut rng = StdRng::seed_from_u64(59);
        let mut pacer = RequestPacer::api_call();
        pacer.record_rate_limit();
        for _ in 0..256 {
            let delay = pacer.next_delay(&mut rng);
            assert!(
                delay >= Duration::from_millis(600) && delay <= Duration::from_millis(2_400),
                "{delay:?} outside doubled API-call bounds"
            );
        }
    }

    /// Regression: `BackoffPolicy::delay` returned the raw saturating power
    /// of two with no ceiling, so a configured base of one hour produced
    /// retry sleeps of hours while the crate documents `MAX_PACING_BACKOFF`
    /// as the hard ceiling on every delay it produces. Scanners must fail
    /// visibly within a minute, not sleep for a session.
    #[test]
    fn backoff_policy_delay_clamps_to_max_pacing_backoff() {
        let policy = BackoffPolicy::new(8, 3_600_000, 3_600_000);
        assert_eq!(
            policy.delay(BackoffKind::RateLimited, 0),
            MAX_PACING_BACKOFF
        );
        assert_eq!(policy.delay(BackoffKind::Timeout, 20), MAX_PACING_BACKOFF);
    }

    /// Regression: `parse_retry_after` accepted a signed delta like `"+5"`
    /// because it parsed the token as `i64`. RFC 9110 delta-seconds is
    /// `1*DIGIT` with no sign. Accepting a signed token drifts from what a
    /// standards-reading server (or WAF probe) sends and from what sibling
    /// parsers accept.
    #[test]
    fn retry_after_rejects_signed_delta_seconds() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(parse_retry_after("+5", now), None);
        assert_eq!(parse_retry_after("5 ", now), Some(Duration::from_secs(5)));
    }

    /// Regression: `parse_retry_after` previously returned `None` when given a
    /// delta-seconds header whose digits overflowed `u64`. A valid long
    /// cooldown requested by a server must saturate and clamp to
    /// `MAX_RETRY_AFTER_OBEYED` rather than silently falling back to zero delay.
    #[test]
    fn retry_after_overflowing_digit_string_clamps_to_max_retry_after() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let huge_digits = "9".repeat(50);
        assert_eq!(
            parse_retry_after(&huge_digits, now),
            Some(MAX_RETRY_AFTER_OBEYED)
        );
    }

    /// Regression: `capped_exponential_backoff` with a `max_ms` exceeding
    /// `MAX_PACING_BACKOFF` (e.g. 5 minutes or `u64::MAX`) leaked the larger
    /// cap instead of clamping to the 60s fleet pacing ceiling.
    #[test]
    fn capped_exponential_backoff_clamps_to_max_pacing_backoff() {
        assert_eq!(
            capped_exponential_backoff(1000, 2.0, 10, 300_000),
            MAX_PACING_BACKOFF
        );
        assert_eq!(
            capped_exponential_backoff(1000, 2.0, 10, u64::MAX),
            MAX_PACING_BACKOFF
        );
    }

    /// Proving test: `BoundedNormalDelay` with `f64::EPSILON` lower bound on Box-Muller $u_1$
    /// allows sampling the full range `[min_ms, max_ms]` including the extreme tails
    /// near `min_ms` and `max_ms`. Under the old `0.001..1.0` range, $Z$ was truncated at
    /// 3.717 $\sigma$, which prevented sampling the outer ~3.5% of the envelope.
    #[test]
    fn bounded_normal_delay_can_sample_full_envelope() {
        let mut rng = StdRng::seed_from_u64(12345);
        let pacer = BoundedNormalDelay::from_unordered_bounds(100, 1_000);
        let mut saw_low = false;
        let mut saw_high = false;

        for _ in 0..100_000 {
            let delay = pacer.sample(&mut rng);
            if delay <= Duration::from_millis(115) {
                saw_low = true;
            }
            if delay >= Duration::from_millis(985) {
                saw_high = true;
            }
            if saw_low && saw_high {
                break;
            }
        }
        assert!(
            saw_low,
            "BoundedNormalDelay should reach the low tail <= 115ms"
        );
        assert!(
            saw_high,
            "BoundedNormalDelay should reach the high tail >= 985ms"
        );
    }

    /// Test inspectability getters on `BackoffPolicy` and `RequestPacer`.
    #[test]
    fn policy_and_pacer_getters_match_constructed_state() {
        let policy = BackoffPolicy::new(5, 400, 150);
        assert_eq!(policy.max_retries(), 5);
        assert_eq!(policy.rate_limited_base_ms(), 400);
        assert_eq!(policy.timeout_base_ms(), 150);

        let pacer = RequestPacer::api_call();
        assert_eq!(pacer.base().min_ms(), 300);
        assert_eq!(pacer.base().max_ms(), 1_200);
    }
}
