# guise-profiles - pure browser fingerprint profile data

[![santh status](https://img.shields.io/badge/santh-beta-blue)](https://santh.dev/standard)

Holds the canonical `StealthProfile` selector, per-profile `ProfileFacts` (User-Agent, navigation headers, hardware display parameters, client hints), and network transport TCP/IP SYN projections. This crate intentionally has no runtime, HTTP, TLS, browser, or async dependencies. It sits below `scanclient` and `stealth` so both crates can derive browser identity from one pure source without creating dependency cycles.

## Quick start

```rust
use guise_profiles::{
    DEFAULT_STEALTH_PROFILE, profile_facts, profile_os_network_stack,
    profile_request_headers, BrowserRequestKind,
};

fn main() {
    let profile = DEFAULT_STEALTH_PROFILE;
    let facts = profile_facts(profile);
    let stack = profile_os_network_stack(profile);
    let headers = profile_request_headers(profile, BrowserRequestKind::DocumentNavigation);

    println!("Profile: {}", facts.user_agent);
    println!("Initial TTL: {}", stack.initial_ttl);
    println!("p0f signature: {}", stack.p0f_signature());
    println!("Header count: {}", headers.as_slice().len());
}
```

## When to use / when not to use

### When to use
- Selecting browser identities and User-Agent strings for HTTP client impersonation.
- Inferring operating system initial TTL and TCP SYN option fingerprints for transport coherence checks.
- Deriving coherent hardware parameters (screen resolution, WebGL vendor/renderer) for browser stealth contexts.

### When not to use
- Managing active socket IO or live TCP connection parameters (use higher-level network/stealth crates).
- Generating dynamic synthetic TLS client hellos (use `scanclient` or `stealth`).
- Parsing unconstrained raw User-Agent strings for non-stealth web analytics (use a dedicated general UA parser).

## Compared to alternatives

Unlike general User-Agent parsing libraries such as `woothehee` or `user_agent`, `guise-profiles` is a pure reference-data catalog designed for stealth protocol stacks. Standard UA parsers focus on extracts like family or version numbers, whereas `guise-profiles` pairs each browser persona with coherent navigation headers, Client Hint brand vectors, hardware attributes, and transport-layer TCP/IP SYN characteristics (initial TTL, MSS, window scale, option layouts, and JA4T signatures).

Compared to embedding raw User-Agent strings directly in transport configurations, `guise-profiles` guarantees compile-time non-emptiness across all profile facts and total platform safety. High-level HTTP and browser automation engines consume these identical profiles to prevent cross-layer fingerprint mismatches (such as advertising a Windows User-Agent over a Linux TCP stack).

## How it fits in Santh

`guise-profiles` lives in `libs/runtime/` as the foundational pure browser fingerprint data layer. It has zero runtime dependencies and sits directly below `scanclient`, `stealth`, and `karyx`. Higher-level scanner transports and browser impersonation layers depend on `guise-profiles` to project coherent HTTP headers and TCP/IP signatures across fleet operations.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
