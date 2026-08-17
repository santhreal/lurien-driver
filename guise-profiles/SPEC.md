# guise-profiles Specification

## Purpose & Scope

`guise-profiles` is the pure reference-data catalog for browser identities and TCP/IP SYN projections within Santh stealth stacks. It has zero runtime, I/O, network, or async dependencies.

## Architecture

The crate consists of two core modules:
1. `crate` (`src/lib.rs`): Top-level identity catalog (`StealthProfile`), per-persona facts (`ProfileFacts`), User-Agent parser/inferer (`user_agent_facts`), client hint vectors (`ProfileClientHintBrand`), header projections (`HeaderProfile`, `BrowserRequestHeaders`), and hardware attributes (`ProfileHardware`).
2. `os_network` (`src/os_network.rs`): Transport-layer TCP/IP SYN projections (`OsNetworkStack`), initial TTL inference (`infer_initial_ttl`), p0f signature generation, and JA4T fingerprint rendering (`ja4t`).

## Invariants & Guarantees

1. **Zero Allocations for Core Lookups**: `profile_facts`, `profile_platform`, `profile_hardware`, `profile_client_hint_brands`, `profile_navigation_headers`, and `profile_os_network_stack` operate entirely on `const` static slices and `const fn` lookups.
2. **Compile-Time Table Non-Emptiness**: Every `StealthProfile` variant's hardware table is verified non-empty at compile time via `const _: () = { ... }` iterating over `ALL_PROFILES`.
3. **Fail-Closed OS Resolution**: `profile_os_network_stack` panics in `const` context if a profile maps to `UserAgentPlatform::Unknown`, preventing silent fallback to a default stack (G017 transport tell).
4. **Single-Owner Window Tokens**: Both `p0f_signature()` and `ja4t()` delegate advertised-window formatting to `OsNetworkStack::window_field()`.
5. **No Guesses on Unmeasurable TTL**: `infer_initial_ttl(0)` returns `0` and yields `NetworkOsCoherence::Unknown`, ensuring unmeasurable wire TTLs do not fabricate coherence evidence.
6. **No Panics on Public API**: All lookup functions (`rotate`, `profile_hardware_at`, `named_profile`, `infer_profile_from_user_agent`) are total and panic-free over arbitrary `usize`, `u8`, or `&str` inputs.

## Public API Contract

- `enum StealthProfile`: `#[non_exhaustive]` persona selector.
- `const ALL_PROFILES: &[StealthProfile]`: Exhaustive list of catalog profiles.
- `const ROTATION_PROFILES: &[StealthProfile]`: Subset suitable for fleet rotation.
- `fn named_profile(&str) -> Option<StealthProfile>`: Case- and whitespace-insensitive string resolver.
- `fn user_agent_facts(&str) -> UserAgentFacts`: Pure UA parser returning browser, platform, major version, and inferred profile.
- `fn profile_facts(StealthProfile) -> ProfileFacts`: Canonical browser identity facts.
- `fn profile_os_network_stack(StealthProfile) -> OsNetworkStack`: TCP/IP SYN parameters (initial TTL, MSS, window scale, option layout).
- `fn infer_initial_ttl(u8) -> u8`: De-hops observed wire TTL to smallest canonical initial TTL (`64`, `128`, `255`).
- `fn os_network_coherence(StealthProfile, u8) -> NetworkOsCoherence`: Coherence verdict between claimed persona OS and observed wire TTL.

## Future Roadmap

- **Tier-B TOML Data Loading**: Support external dropped-in TOML persona/hardware data files under `rules/personas/` for runtime profile extensions while retaining zero-alloc compiled defaults.
