//! Uniform random selection and stealth-safe token primitives.
//!
//! This crate owns the empty-domain and alphabet contracts for random choices
//! used by stealth consumers. The APIs avoid modulo-biased sampling and return
//! `None` for invalid caller-supplied alphabets instead of panicking.

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

use rand::{Rng, SeedableRng};

pub use rand::rngs::StdRng;

/// A deterministic 32-byte seed for fleet-wide RNGs.
///
/// Using a fixed-size byte seed keeps persona reproducibility independent of any
/// particular RNG version: a seed stored in an incident log can rebuild the exact
/// same persona/behavior/trace years later as long as the generation logic is
/// preserved.
pub type Seed = [u8; 32];

/// Convert a small integer seed into a full byte seed deterministically.
///
/// This is the canonical bridge from human-readable seeds (request IDs, session
/// counters) to the byte-oriented RNG constructor.
#[must_use]
pub fn seed_from_u64(seed: u64) -> Seed {
    let mut out = [0u8; 32];
    let mut x = seed.wrapping_add(0x9e3779b97f4a7c15);
    for chunk in out.chunks_mut(8) {
        x = splitmix64(x);
        chunk.copy_from_slice(&x.to_le_bytes());
    }
    out
}

/// Build a seeded `StdRng` from a byte seed.
#[must_use]
pub fn seeded_rng(seed: &Seed) -> StdRng {
    StdRng::from_seed(*seed)
}

/// Build a seeded `StdRng` from a small integer seed.
#[must_use]
pub fn seeded_rng_from_u64(seed: u64) -> StdRng {
    seeded_rng(&seed_from_u64(seed))
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Lowercase ASCII letters plus digits.
///
/// This alphabet is suitable for DNS-label components, nonce suffixes, and
/// opaque tokens that should avoid punctuation and case-sensitive ambiguity.
pub const LOWER_ALPHANUMERIC: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Sample a uniformly random index in `0..len`.
///
/// Returns `None` for an empty domain instead of panicking.
#[must_use]
pub fn random_index(len: usize) -> Option<usize> {
    let mut rng = rand::thread_rng();
    random_index_with_rng(len, &mut rng)
}

/// Sample a uniformly random index in `0..len` using the caller's RNG.
///
/// Returns `None` for an empty domain instead of panicking.
#[must_use]
pub fn random_index_with_rng<R: Rng + ?Sized>(len: usize, rng: &mut R) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(rng.gen_range(0..len))
}

/// Sample a uniformly random item from a slice.
///
/// Returns `None` for an empty slice instead of panicking.
#[must_use]
pub fn random_item<T>(items: &[T]) -> Option<&T> {
    let mut rng = rand::thread_rng();
    random_item_with_rng(items, &mut rng)
}

/// Sample a uniformly random item from a slice using the caller's RNG.
///
/// Returns `None` for an empty slice instead of panicking.
#[must_use]
pub fn random_item_with_rng<'a, T, R: Rng + ?Sized>(items: &'a [T], rng: &mut R) -> Option<&'a T> {
    random_index_with_rng(items.len(), rng).map(|index| &items[index])
}

/// Return `true` with the requested probability using thread-local entropy.
///
/// Probabilities at or below zero, including `NaN`, always return `false`.
/// Probabilities at or above one always return `true`.
#[must_use]
pub fn chance(probability: f64) -> bool {
    let mut rng = rand::thread_rng();
    chance_with_rng(probability, &mut rng)
}

/// Return `true` with the requested probability using the caller's RNG.
///
/// Probabilities at or below zero, including `NaN`, always return `false`.
/// Probabilities at or above one always return `true`.
#[must_use]
pub fn chance_with_rng<R: Rng + ?Sized>(probability: f64, rng: &mut R) -> bool {
    if probability.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return false;
    }
    if probability >= 1.0 {
        return true;
    }
    rng.gen::<f64>() < probability
}

/// Sample an index from a non-negative finite weight slice.
///
/// Zero, negative, NaN, and infinite weights are ignored. Returns `None` when
/// no item has a usable positive finite weight.
#[must_use]
pub fn weighted_index(weights: &[f64]) -> Option<usize> {
    let mut rng = rand::thread_rng();
    weighted_index_with_rng(weights, &mut rng)
}

/// Sample an index from a non-negative finite weight slice using the caller's RNG.
///
/// Zero, negative, NaN, and infinite weights are ignored. Returns `None` when
/// no item has a usable positive finite weight.
#[must_use]
pub fn weighted_index_with_rng<R: Rng + ?Sized>(weights: &[f64], rng: &mut R) -> Option<usize> {
    weighted_index_by_with_rng(weights, |weight| *weight, rng)
}

/// Sample an item index using weights projected from each item.
///
/// The projection is evaluated twice, once for the total and once for
/// selection, so it must be deterministic for a given item. Zero, negative,
/// NaN, and infinite weights are ignored. Returns `None` when no item has a
/// usable positive finite weight.
#[must_use]
pub fn weighted_index_by_with_rng<T, R, F>(items: &[T], weight: F, rng: &mut R) -> Option<usize>
where
    R: Rng + ?Sized,
    F: Fn(&T) -> f64,
{
    let total = total_usable_weight(items.iter().map(&weight))?;
    let mut ticket = rng.gen_range(0.0..total);
    let mut fallback = None;

    for (index, item) in items.iter().enumerate() {
        let item_weight = weight(item);
        if !is_usable_weight(item_weight) {
            continue;
        }
        fallback = Some(index);
        if ticket < item_weight {
            return Some(index);
        }
        ticket -= item_weight;
    }

    fallback
}

/// Generate a lowercase alphanumeric string using thread-local entropy.
#[must_use]
pub fn random_lower_alphanumeric(len: usize) -> String {
    let mut rng = rand::thread_rng();
    random_lower_alphanumeric_with_rng(len, &mut rng)
}

/// Generate a lowercase alphanumeric string using the caller's RNG.
#[must_use]
pub fn random_lower_alphanumeric_with_rng<R: Rng + ?Sized>(len: usize, rng: &mut R) -> String {
    sample_ascii_string(len, LOWER_ALPHANUMERIC, rng)
}

/// Generate an ASCII string from a caller-supplied alphabet using thread-local entropy.
///
/// Returns `None` when `alphabet` is empty or contains a non-ASCII byte.
#[must_use]
pub fn random_ascii_string(len: usize, alphabet: &[u8]) -> Option<String> {
    let mut rng = rand::thread_rng();
    random_ascii_string_with_rng(len, alphabet, &mut rng)
}

/// Generate an ASCII string from a caller-supplied alphabet using the caller's RNG.
///
/// Returns `None` when `alphabet` is empty or contains a non-ASCII byte.
#[must_use]
pub fn random_ascii_string_with_rng<R: Rng + ?Sized>(
    len: usize,
    alphabet: &[u8],
    rng: &mut R,
) -> Option<String> {
    if alphabet.is_empty() || alphabet.iter().any(|byte| !byte.is_ascii()) {
        return None;
    }
    Some(sample_ascii_string(len, alphabet, rng))
}

fn sample_ascii_string<R: Rng + ?Sized>(len: usize, alphabet: &[u8], rng: &mut R) -> String {
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let Some(index) = random_index_with_rng(alphabet.len(), rng) else {
            return out;
        };
        out.push(char::from(alphabet[index]));
    }
    out
}

fn total_usable_weight<I>(weights: I) -> Option<f64>
where
    I: IntoIterator<Item = f64>,
{
    let mut total = 0.0;
    for weight in weights {
        if is_usable_weight(weight) {
            total += weight;
            if !total.is_finite() {
                return None;
            }
        }
    }
    (total > 0.0).then_some(total)
}

fn is_usable_weight(weight: f64) -> bool {
    weight.is_finite() && weight > 0.0
}

#[cfg(test)]
mod tests {
    use super::{
        chance, chance_with_rng, random_ascii_string_with_rng, random_index, random_index_with_rng,
        random_item, random_item_with_rng, random_lower_alphanumeric,
        random_lower_alphanumeric_with_rng, seed_from_u64, seeded_rng_from_u64, weighted_index,
        weighted_index_by_with_rng, weighted_index_with_rng, LOWER_ALPHANUMERIC,
    };
    use rand::{Rng, SeedableRng};

    #[test]
    fn random_index_rejects_empty_domain() {
        assert_eq!(random_index(0), None);
    }

    #[test]
    fn random_index_stays_bounded() {
        for len in 1..16 {
            for _ in 0..128 {
                let index = random_index(len).expect("non-empty domain should produce an index");
                assert!(index < len);
            }
        }
    }

    #[test]
    fn random_index_with_rng_rejects_empty_domain() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        assert_eq!(random_index_with_rng(0, &mut rng), None);
    }

    #[test]
    fn random_index_with_rng_stays_bounded() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        for len in 1..16 {
            for _ in 0..128 {
                let index = random_index_with_rng(len, &mut rng)
                    .expect("non-empty domain should produce an index");
                assert!(index < len);
            }
        }
    }

    #[test]
    fn random_item_rejects_empty_slice() {
        assert_eq!(random_item::<u8>(&[]), None);
    }

    #[test]
    fn random_item_with_rng_returns_slice_member() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(19);
        let items = ["alpha", "bravo", "charlie"];
        for _ in 0..128 {
            let item = random_item_with_rng(&items, &mut rng)
                .expect("non-empty slice should produce an item");
            assert!(items.contains(item));
        }
    }

    #[test]
    fn chance_rejects_non_positive_and_nan_probabilities() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(29);
        assert!(!chance(-1.0));
        assert!(!chance_with_rng(0.0, &mut rng));
        assert!(!chance_with_rng(f64::NAN, &mut rng));
        assert!(!chance_with_rng(f64::NEG_INFINITY, &mut rng));
    }

    #[test]
    fn chance_accepts_certain_and_overfull_probabilities() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(31);
        assert!(chance_with_rng(1.0, &mut rng));
        assert!(chance_with_rng(7.0, &mut rng));
        assert!(chance_with_rng(f64::INFINITY, &mut rng));
    }

    #[test]
    fn chance_tracks_probability() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(33);
        let mut hits = 0usize;
        for _ in 0..10_000 {
            if chance_with_rng(0.25, &mut rng) {
                hits += 1;
            }
        }
        assert!(
            (2_350..=2_650).contains(&hits),
            "0.25 probability should produce about 2500 hits, got {hits}"
        );
    }

    #[test]
    fn weighted_index_rejects_empty_or_unusable_domains() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(21);
        assert_eq!(weighted_index(&[]), None);
        assert_eq!(
            weighted_index_with_rng(&[0.0, -1.0, f64::NAN, f64::INFINITY], &mut rng),
            None
        );
    }

    #[test]
    fn weighted_index_ignores_unusable_weights() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(23);
        for _ in 0..256 {
            assert_eq!(
                weighted_index_with_rng(&[0.0, f64::NAN, 7.0, -2.0], &mut rng),
                Some(2)
            );
        }
    }

    #[test]
    fn weighted_index_by_projects_item_weights() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(25);
        let items = [
            ("ignored", 0.0),
            ("chosen", 4.0),
            ("also_ignored", f64::NAN),
        ];
        assert_eq!(
            weighted_index_by_with_rng(&items, |(_, weight)| *weight, &mut rng),
            Some(1)
        );
    }

    #[test]
    fn weighted_index_tracks_weight_ratio() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(27);
        let mut second = 0usize;
        for _ in 0..10_000 {
            if weighted_index_with_rng(&[1.0, 3.0], &mut rng) == Some(1) {
                second += 1;
            }
        }
        assert!(
            (7_200..=7_800).contains(&second),
            "3:1 weighted sampling should choose index 1 about 75% of the time, got {second}"
        );
    }

    #[test]
    fn lower_alphanumeric_allows_empty_output() {
        assert_eq!(random_lower_alphanumeric(0), "");
    }

    #[test]
    fn lower_alphanumeric_uses_dns_safe_alphabet() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        let value = random_lower_alphanumeric_with_rng(256, &mut rng);
        assert_eq!(value.len(), 256);
        assert!(value.bytes().all(|byte| LOWER_ALPHANUMERIC.contains(&byte)));
    }

    #[test]
    fn ascii_string_rejects_empty_alphabet() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(13);
        assert_eq!(random_ascii_string_with_rng(8, b"", &mut rng), None);
    }

    #[test]
    fn ascii_string_rejects_non_ascii_alphabet() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(15);
        assert_eq!(random_ascii_string_with_rng(8, &[0xff], &mut rng), None);
    }

    #[test]
    fn ascii_string_uses_caller_alphabet() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(17);
        let value = random_ascii_string_with_rng(128, b"ab7", &mut rng)
            .expect("valid alphabet should generate a string");
        assert_eq!(value.len(), 128);
        assert!(value.bytes().all(|byte| matches!(byte, b'a' | b'b' | b'7')));
    }

    #[test]
    fn seed_from_u64_is_deterministic() {
        let a = seed_from_u64(42);
        let b = seed_from_u64(42);
        assert_eq!(a, b);
        assert_ne!(a, seed_from_u64(43));
    }

    #[test]
    fn seeded_rng_from_u64_reproduces_samples() {
        let seed = 7;
        let mut rng_a = seeded_rng_from_u64(seed);
        let mut rng_b = seeded_rng_from_u64(seed);
        assert_eq!(rng_a.gen::<u64>(), rng_b.gen::<u64>());
        assert_eq!(
            random_index_with_rng(100, &mut rng_a),
            random_index_with_rng(100, &mut rng_b)
        );
    }

    #[test]
    fn seed_from_u64_expands_to_full_32_bytes() {
        let seed = seed_from_u64(12345);
        assert!(
            seed.iter().any(|b| *b != 0),
            "seed should expand beyond zeros"
        );
    }
    #[test]
    fn ascii_string_rejects_invalid_alphabets_even_on_zero_len() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(19);
        assert_eq!(random_ascii_string_with_rng(0, b"", &mut rng), None);
        assert_eq!(random_ascii_string_with_rng(0, &[0x80], &mut rng), None);
        assert_eq!(
            random_ascii_string_with_rng(0, b"abc", &mut rng),
            Some(String::new())
        );
    }

    #[test]
    fn weighted_index_accepts_single_maximal_finite_weight() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(21);
        assert_eq!(weighted_index_with_rng(&[f64::MAX], &mut rng), Some(0));
    }

    #[test]
    fn weighted_index_ignores_infinite_weights() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(23);
        assert_eq!(
            weighted_index_with_rng(&[f64::INFINITY, f64::NEG_INFINITY], &mut rng),
            None
        );
        assert_eq!(
            weighted_index_with_rng(&[f64::INFINITY, 5.0, f64::NEG_INFINITY], &mut rng),
            Some(1)
        );
    }

    #[test]
    fn chance_handles_subnormal_positive_probability() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(25);
        let _res = chance_with_rng(f64::MIN_POSITIVE, &mut rng);
    }
}
