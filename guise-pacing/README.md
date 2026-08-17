# guise-pacing - pure retry backoff, jitter, and Retry-After pacing primitives

[![santh status](https://img.shields.io/badge/santh-alpha-orange)](https://santh.dev/standard)

`guise-pacing` provides pure, `async`-decoupled retry backoff, jitter, request pacing, and `Retry-After` header parsing primitives for the Santh fleet. It contains no network or I/O runtime code, keeping timing calculations deterministic, safe, and easily testable across HTTP client layers, browser automation drivers, and scanners.

## Quick start

```rust
use std::time::{Duration, SystemTime};
use guise_pacing::{
    BackoffPolicy, BackoffKind, RequestPacer, Jitter, BoundedNormalDelay,
    capped_pow2_backoff, parse_retry_after, jittered_backoff,
};

fn main() {
    // 1. Exponential retry backoff
    let policy = BackoffPolicy::gossan_compatible();
    let delay = policy.delay(BackoffKind::RateLimited, 0);
    assert_eq!(delay, Duration::from_millis(500));

    // 2. Capped power-of-two exponential delay
    let pow2_delay = capped_pow2_backoff(Duration::from_millis(100), 3, Duration::from_secs(10));
    assert_eq!(pow2_delay, Duration::from_millis(800));

    // 3. Deterministic nonce-keyed jitter (±20%)
    let jittered = jittered_backoff(Duration::from_millis(1000), 42);
    assert!(jittered >= Duration::from_millis(800) && jittered <= Duration::from_millis(1200));

    // 4. Parse Retry-After HTTP response header
    let retry_after = parse_retry_after("120", SystemTime::now());
    // Capped at MAX_RETRY_AFTER_OBEYED (60s)
    assert_eq!(retry_after, Some(Duration::from_secs(60)));

    // 5. Human-like request pacer
    let mut pacer = RequestPacer::api_call();
    pacer.record_rate_limit(); // Multiplier increases on 429 / 403
    assert_eq!(pacer.challenge_multiplier(), 2);
}
```

## When to use / when not to use

### When to use
- Calculating exponential backoff delays for rate limits (HTTP 429/403) and transport timeouts.
- Applying deterministic or randomized symmetric jitter to retry intervals.
- Parsing RFC 9110 `Retry-After` response header delta-seconds and HTTP-dates safely.
- Simulating human inter-request pacing and think time with bounded normal distributions.

### When not to use
- Executing thread sleeps or managing async timers directly (callers execute the `Duration` delay returned).
- Complex tokio/async channel backpressure (use channel primitives or dedicated async schedulers).

## Compared to alternatives

Unlike `backoff` or `tokio-retry`, `guise-pacing` is a pure timing math library without async runtime or IO bounds. It guarantees hard fleet safety bounds (`MAX_PACING_BACKOFF = 60s`, `MAX_RETRY_AFTER_OBEYED = 60s`) and saturating integer arithmetic so misconfigured base delays or extreme attempt counts never cause integer overflow panics or multi-year hangs.

## How it fits in Santh

`guise-pacing` lives in `libs/runtime/` as the foundational pure timing and retry pacing primitive. High-level Santh HTTP engines (`scanclient`, `stealth`), web drivers (`foxdriver`), and scanner orchestrators depend on `guise-pacing` to share consistent backoff schedules and human-like request pacing contracts across the fleet.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
