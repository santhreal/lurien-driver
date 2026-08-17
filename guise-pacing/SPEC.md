# guise-pacing Specification

## Purpose & Scope

`guise-pacing` is the pure timing, backoff, jitter, request pacing, and `Retry-After` header parsing crate for the Santh fleet. It contains no I/O, network, thread sleep, or async dependencies, ensuring all pacing calculations are pure, deterministic, saturating, and safe against hostile or corrupt policy inputs.

## Architecture

The crate is structured as a single pure library module (`src/lib.rs`):

1. **Randomized & Bounded Delay Envelopes**:
   - `Jitter`: Uniform delay sampled from `min_ms..max_exclusive_ms`.
   - `BoundedNormalDelay`: Box-Muller normal delay centered between bounds with $\sigma = \text{range}/4$, clamped to `[min, max]`.
   - `percent_jitter` / `percent_jitter_with_rng`: Symmetric percent jitter applied to a base `Duration`.

2. **Exponential Backoff Calculations**:
   - `BackoffPolicy`: Fleet retry policy with attempt caps and distinct rate-limit (`BACKOFF_429_BASE_MS = 500ms`) vs timeout (`BACKOFF_TIMEOUT_BASE_MS = 200ms`) base delays.
   - `capped_exponential_backoff`: Floating-point power multiplier backoff with fallback for non-finite / sub-unit multipliers.
   - `capped_pow2_backoff` / `capped_pow2_backoff_ms`: Power-of-two exponential growth for `Duration` and millisecond inputs.

3. **Protocol Parsing & Nonce Jitter**:
   - `parse_retry_after`: Robust RFC 9110 delta-seconds and HTTP-date header parser capped at `MAX_RETRY_AFTER_OBEYED`.
   - `jittered_backoff`: Deterministic ±20% pseudo-random jitter derived from a 32-bit nonce using a bit-shift mixer.

4. **Human-like Request Pacing**:
   - `RequestPacer`: Inter-request delay pacer combining `BoundedNormalDelay` with adaptive challenge penalty multipliers (scaling up on 429/403 signals up to `MAX_CHALLENGE_MULTIPLIER = 16`, decaying on 2xx successes).

## Invariants & Guarantees

1. **Hard Ceiling Clamp (`MAX_PACING_BACKOFF = 60s`)**: All backoff and delay computations clamp output to 60 seconds. Pathological inputs (e.g. `u64::MAX` attempt counts) cannot saturate to multi-year hangs.
2. **Retry-After Obedience Cap (`MAX_RETRY_AFTER_OBEYED = 60s`)**: `parse_retry_after` caps server-named cooldowns to 60 seconds; invalid values, past dates, or negative durations return `None`.
3. **Non-Panic & Overflow Safety**: All attempt counters and nanosecond calculations use saturating arithmetic (`saturating_add`, `saturating_mul`).
4. **Hostile Multiplier Sanitization**: Non-finite (NaN, infinity) or sub-unit ($< 1.0$) multipliers in `capped_exponential_backoff` fall back to `1.0`.
5. **Unordered Bounds Normalization**: `BoundedNormalDelay::from_unordered_bounds` accepts bounds in any order without panicking.
6. **Zero-Delay Invariant**: Any calculation with a zero base duration or zero max duration returns `Duration::ZERO`.

## Public API Contract

- `const BACKOFF_MAX_RETRIES: u32 = 4`
- `const BACKOFF_429_BASE_MS: u64 = 500`
- `const BACKOFF_TIMEOUT_BASE_MS: u64 = 200`
- `const MAX_RETRY_AFTER_OBEYED: Duration = Duration::from_secs(60)`
- `const MAX_PACING_BACKOFF: Duration = Duration::from_secs(60)`
- `struct Jitter`: `zero()`, `up_to(u64)`, `range(u64, u64)`, `min_ms()`, `max_exclusive_ms()`, `sample(&mut R)`, `sample_thread()`
- `struct BoundedNormalDelay`: `zero()`, `from_unordered_bounds(u64, u64)`, `min_ms()`, `max_ms()`, `sample(&mut R)`, `sample_thread()`
- `struct BackoffPolicy`: `new(u32, u64, u64)`, `gossan_compatible()`, `max_retries()`, `rate_limited_base_ms()`, `timeout_base_ms()`, `should_retry_after(u32)`, `delay(BackoffKind, u32)`
- `enum BackoffKind`: `RateLimited`, `Timeout`
- `struct RequestPacer`: `new(BoundedNormalDelay)`, `page_load()`, `sub_resource()`, `api_call()`, `base()`, `challenge_multiplier()`, `record_success()`, `record_rate_limit()`, `record_http_status(u16)`, `next_delay(&mut R)`, `next_delay_thread()`
- `fn percent_jitter(Duration, u64) -> Duration`
- `fn percent_jitter_with_rng<R: Rng + ?Sized>(Duration, u64, &mut R) -> Duration`
- `fn capped_exponential_backoff(u64, f64, u32, u64) -> Duration`
- `fn capped_pow2_backoff(Duration, u32, Duration) -> Duration`
- `fn capped_pow2_backoff_ms(u64, u32, u64) -> Duration`
- `fn parse_retry_after(&str, SystemTime) -> Option<Duration>`
- `fn jittered_backoff(Duration, u32) -> Duration`
- `enum PacingError`: `InvalidJitterRange { min_ms: u64, max_exclusive_ms: u64 }`

## Future Roadmap

- **`no_std` Feature Support**: Make `rand` optional or pluggable to support bare-metal/embedded pacing targets.
