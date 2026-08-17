# Technical Specification: `guise-choice`

## 1. Overview & Scope

`guise-choice` defines uniform selection, weighted indexing, persona seed derivation, and ASCII token generation primitives for the Santh stealth runtime stack.

It lives in the `libs/runtime/` layer of the Santh monorepo as an `alpha` crate. It provides panic-free guarantees over caller inputs, eliminating common panic sources (such as empty domain sizes, non-ASCII alphabets, NaN weights, or integer overflow) while ensuring persona reproducibility across incident logs.

---

## 2. Primitives & Algorithm Specification

### 2.1 Seed Expansion & RNG Bridge
- `Seed`: `[u8; 32]` fixed byte array representation for 256-bit RNG states.
- `seed_from_u64(seed: u64) -> Seed`: Expands a 64-bit integer into a 32-byte seed by applying 4 steps of `splitmix64` mixed with the Golden Ratio constant (`0x9e3779b97f4a7c15`).
- `seeded_rng_from_u64(seed: u64) -> StdRng`: Instantiates an `StdRng` using the expanded 32-byte seed.

### 2.2 Uniform Index & Item Sampling
- `random_index_with_rng(len: usize, rng: &mut R) -> Option<usize>`: Evaluates `len`. If `len == 0`, returns `None`. Otherwise returns `Some(rng.gen_range(0..len))`.
- `random_item_with_rng(items: &[T], rng: &mut R) -> Option<&T>`: Delegates to `random_index_with_rng(items.len(), rng)` and indexes `items`.

### 2.3 Bernoulli / Chance Gates
- `chance_with_rng(probability: f64, rng: &mut R) -> bool`:
  - `probability <= 0.0` or `NaN` -> `false`.
  - `probability >= 1.0` -> `true`.
  - `0.0 < probability < 1.0` -> `rng.gen::<f64>() < probability`.

### 2.4 Weighted Index Sampling
- `weighted_index_by_with_rng(items: &[T], weight: F, rng: &mut R) -> Option<usize>`:
  - Traverses `items` to calculate `total_usable_weight`. Ignores non-finite (`NaN`, `inf`), zero, and negative weights. Returns `None` if sum overflows `f64` or no positive usable weight exists.
  - Draws a ticket `t` uniformly in `0.0..total_usable_weight`.
  - Performs a linear scan over usable items, subtracting weights until `t < item_weight`.
  - Employs a fallback tracker to guarantee returning `Some(last_usable_index)` if floating-point accumulation rounding causes `t >= sum(weights)`.

### 2.5 ASCII String Generation
- `random_lower_alphanumeric_with_rng(len, rng)`: Samples characters from `LOWER_ALPHANUMERIC` (`b"abcdefghijklmnopqrstuvwxyz0123456789"`).
- `random_ascii_string_with_rng(len, alphabet, rng)`: Validates that `alphabet` is non-empty and all bytes are ASCII (`byte.is_ascii()`). Returns `None` if invalid.

---

## 3. Invariants & Mathematical Guarantees

1. **Empty-Domain Panic Freedom**:
   All public sampling functions accept `len == 0` or empty slice inputs gracefully by returning `None` (or empty string for zero-length token requests), preventing unexpected runtime crashes.

2. **Strict Usable Weight Filter**:
   `weighted_index` filters out `0.0`, `-0.0`, negative numbers, `f64::NAN`, and `f64::INFINITY`. Sampling only proceeds over finite positive weights. If total weight overflows `f64::MAX`, the function fails closed by returning `None`.

3. **Injective 64-bit Seed Expansion**:
   `seed_from_u64` uses `splitmix64` to map distinct 64-bit integer seeds to distinct 32-byte persona seeds, preventing persona collision during seeded stream initialization.

4. **Multi-pass Projection Evaluation Gap**:
   `weighted_index_by_with_rng` evaluates the weight projection closure twice: once to calculate total usable weight and once to select the winning index. The projection must be deterministic.

---

## 4. Public API Contract

### Exports (`guise_choice::*`)
- Re-exports: `rand::rngs::StdRng`.
- Constants: `LOWER_ALPHANUMERIC`.
- Types: `Seed`.
- Functions: `seed_from_u64`, `seeded_rng`, `seeded_rng_from_u64`, `random_index`, `random_index_with_rng`, `random_item`, `random_item_with_rng`, `chance`, `chance_with_rng`, `weighted_index`, `weighted_index_with_rng`, `weighted_index_by_with_rng`, `random_lower_alphanumeric`, `random_lower_alphanumeric_with_rng`, `random_ascii_string`, `random_ascii_string_with_rng`.

---

## 5. Verification & Test Strategy

- **`src/lib.rs` (`mod tests`)**: Unit tests validating bounds, zero-length domains, weighted ratio statistics, chance gate thresholds, and seed expansion.
- **`tests/property.rs`**: Proptest suite proving bounds invariants, probability clamping, usable weight filtering, non-colliding seed expansion, and reproducible persona sequences.
- **`tests/gap.rs`**: Pins documented contract gaps (weight sum overflow `None` return, multi-pass projection call count range `N < calls <= 2N`, duplicate alphabet multiset bias, DNS alphabet purity).
