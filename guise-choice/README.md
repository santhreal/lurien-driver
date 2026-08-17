# guise-choice - uniform random selection and stealth-safe token primitives

[![santh status](https://img.shields.io/badge/santh-alpha-orange)](https://santh.dev/standard)

`guise-choice` provides uniform random selection, weighted index sampling, probability chance gates, seed expansion, and stealth-safe ASCII token primitives for the Santh fleet.

The crate enforces empty-domain safety (returning `Option` instead of panicking on empty slices), eliminates modulo-bias in selection, filters invalid/non-finite weights during weighted sampling, and provides deterministic 32-byte persona seed derivation.

## Quick Start

```rust
use guise_choice::{
    chance, random_item, random_lower_alphanumeric, seeded_rng_from_u64, weighted_index_with_rng,
};

// 1. Safe slice sampling (returns None on empty, never panics)
let items = ["alpha", "bravo", "charlie"];
if let Some(item) = random_item(&items) {
    println!("Selected item: {item}");
}

// 2. Persona seed expansion for reproducible RNG streams
let mut rng = seeded_rng_from_u64(42);

// 3. Weighted index sampling (ignores 0.0, NaN, infinity, and negative weights)
let weights = [0.0, 3.0, f64::NAN, 1.0];
let choice = weighted_index_with_rng(&weights, &mut rng);
assert_eq!(choice, Some(1));

// 4. DNS-safe lower alphanumeric token generation
let token = random_lower_alphanumeric(16);
assert_eq!(token.len(), 16);

// 5. Probability chance check
if chance(0.25) {
    println!("Triggered 25% chance gate");
}
```

## When to use / when not to use

- **Use when:**
  - You need panic-free random choice over dynamic slices, domains, or candidate lists.
  - You are building persona-seeded RNG streams or reproducible session nonces.
  - You perform weighted choice over floating-point distributions (e.g. stealth delays or headers).
  - You generate DNS-safe or custom-alphabet ASCII tokens.
- **Do not use when:**
  - You require cryptographically secure key material or cryptographic nonces (use OS entropy / `ring`).
  - You need complex browser persona profile generation (use `guise-profiles` instead).

## Compared to alternatives

- **Modulo indexing vs `random_index`**: Naive `rng.gen::<usize>() % len` panics on `len == 0` and introduces modulo bias. `guise-choice` handles `len == 0` safely via `Option` and leverages `gen_range` to prevent distribution skew.
- **Strict weighted sampling vs `weighted_index`**: Standard weighted index constructs panic or return errors on `NaN`, zero, or negative weights. `guise-choice` filters invalid weights gracefully and returns `None` if no positive finite weight exists.

## How it fits in Santh

`guise-choice` lives in the `libs/runtime/` layer of the Santh monorepo. It serves as a zero-panic utility library relied on by high-level stealth orchestration (`guise`), fingerprint surface generators (`guise-profiles`), and runtime automation engines.

## License

MIT OR Apache-2.0
