//! Property-based tests for guise-choice.
//!
//! These pin the sampling invariants the stealth consumers rely on for *any*
//! input: indices stay in range, unusable weights are never selected, and
//! seeded generation is reproducible, not just for hand-picked cases.

use guise_choice::{
    chance_with_rng, random_ascii_string_with_rng, random_index_with_rng, random_item_with_rng,
    seed_from_u64, seeded_rng_from_u64, weighted_index_with_rng,
};
use proptest::prelude::*;

fn is_usable(weight: f64) -> bool {
    weight.is_finite() && weight > 0.0
}

proptest! {
    /// For every domain size and seed, `random_index_with_rng` returns `None`
    /// exactly for the empty domain and otherwise stays in bounds.
    #[test]
    fn random_index_bounds_for_any_domain(len in 0usize..=10_000, seed in any::<u64>()) {
        let mut rng = seeded_rng_from_u64(seed);
        let index = random_index_with_rng(len, &mut rng);
        prop_assert_eq!(index.is_none(), len == 0);
        if let Some(index) = index {
            prop_assert!(index < len);
        }
    }

    /// A sampled item is always a member of the input slice.
    #[test]
    fn random_item_always_a_member(
        items in prop::collection::vec(any::<u64>(), 0..64),
        seed in any::<u64>(),
    ) {
        let mut rng = seeded_rng_from_u64(seed);
        let item = random_item_with_rng(&items, &mut rng);
        prop_assert_eq!(item.is_none(), items.is_empty());
        if let Some(item) = item {
            prop_assert!(items.contains(item));
        }
    }

    /// Probability clamps are total: at or below zero (including NaN) always
    /// false, at or above one always true, for any seed.
    #[test]
    fn chance_clamps_hostile_probabilities(
        probability in prop_oneof![
            Just(f64::NAN),
            Just(f64::NEG_INFINITY),
            Just(f64::INFINITY),
            any::<f64>(),
        ],
        seed in any::<u64>(),
    ) {
        let mut rng = seeded_rng_from_u64(seed);
        let outcome = chance_with_rng(probability, &mut rng);
        if probability.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            prop_assert!(!outcome, "{probability} must never fire");
        } else if probability >= 1.0 {
            prop_assert!(outcome, "{probability} must always fire");
        }
    }

    /// Weighted sampling never selects a zero, negative, NaN, or infinite
    /// weight, and returns `None` exactly when no usable weight exists or
    /// the usable weights overflow the f64 sum.
    #[test]
    fn weighted_index_never_picks_unusable_weight(
        weights in prop::collection::vec(
            prop_oneof![
                0.0f64..1_000_000.0,
                Just(f64::NAN),
                Just(f64::INFINITY),
                Just(-1.0),
                Just(0.0),
                Just(f64::MAX),
            ],
            0..16,
        ),
        seed in any::<u64>(),
    ) {
        let mut rng = seeded_rng_from_u64(seed);
        let index = weighted_index_with_rng(&weights, &mut rng);
        match index {
            Some(index) => {
                prop_assert!(index < weights.len());
                prop_assert!(is_usable(weights[index]), "picked unusable weight {}", weights[index]);
            }
            None => {
                let any_usable = weights.iter().any(|weight| is_usable(*weight));
                let usable_sum: f64 = weights.iter().copied().filter(|weight| is_usable(*weight)).sum();
                prop_assert!(!any_usable || !usable_sum.is_finite(),
                    "None despite usable weights {weights:?}");
            }
        }
    }

    /// ASCII-string generation honors the alphabet contract for arbitrary
    /// caller alphabets: `None` exactly when the alphabet is empty or has a
    /// non-ASCII byte, else the output length and membership hold.
    #[test]
    fn ascii_string_respects_alphabet_contract(
        alphabet in prop::collection::vec(any::<u8>(), 0..32),
        len in 0usize..64,
        seed in any::<u64>(),
    ) {
        let mut rng = seeded_rng_from_u64(seed);
        let value = random_ascii_string_with_rng(len, &alphabet, &mut rng);
        let valid = !alphabet.is_empty() && alphabet.iter().all(u8::is_ascii);
        prop_assert_eq!(value.is_some(), valid);
        if let Some(value) = value {
            prop_assert_eq!(value.len(), len);
            prop_assert!(value.bytes().all(|byte| alphabet.contains(&byte)));
        }
    }

    /// Seed expansion is deterministic and injective over u64 seeds: two
    /// distinct seeds must not collapse to the same 32-byte persona seed
    /// (splitmix64 is a bijection; a regression here would silently merge
    /// personas).
    #[test]
    fn seed_from_u64_deterministic_and_injective(first in any::<u64>(), second in any::<u64>()) {
        prop_assert_eq!(seed_from_u64(first), seed_from_u64(first));
        if first != second {
            prop_assert_ne!(seed_from_u64(first), seed_from_u64(second));
        }
    }

    /// Same seed, same sample sequence: persona reproducibility across runs.
    #[test]
    fn seeded_sampling_is_reproducible(len in 1usize..1000, seed in any::<u64>()) {
        let mut rng_a = seeded_rng_from_u64(seed);
        let mut rng_b = seeded_rng_from_u64(seed);
        for _ in 0..8 {
            prop_assert_eq!(
                random_index_with_rng(len, &mut rng_a),
                random_index_with_rng(len, &mut rng_b)
            );
        }
    }
}
