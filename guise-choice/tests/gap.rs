//! Gap tests: documented limitations and deliberate behaviors of
//! guise-choice that must stay pinned so any future change is a conscious
//! decision, not silent drift.

use guise_choice::{
    seeded_rng_from_u64, weighted_index_by_with_rng, weighted_index_with_rng, LOWER_ALPHANUMERIC,
};

/// GAP: weights that are each individually usable (positive, finite) but
/// whose sum overflows f64 (`f64::MAX + f64::MAX`) make the whole domain
/// unusable and the sampler returns `None` instead of normalizing. A
/// normalized sample would be well-defined; the current contract fails
/// closed. Pinned so switching to normalized sampling is a deliberate
/// change with a changelog entry.
#[test]
fn usable_weights_with_overflowing_sum_yield_none() {
    let mut rng = seeded_rng_from_u64(7);
    assert_eq!(
        weighted_index_with_rng(&[f64::MAX, f64::MAX], &mut rng),
        None
    );
}

/// GAP: `weighted_index_by_with_rng` evaluates the projection more than once
/// per item: a full pass for the total, then a second pass for selection
/// that short-circuits at the winning item. A side-effecting projection is
/// therefore observed between N and 2N times for N items, never exactly
/// once per item. Pinned so a future single-evaluation rewrite is a
/// deliberate change.
#[test]
fn weighted_projection_is_evaluated_at_least_once_per_item() {
    let items = [1.0f64, 2.0, 3.0];
    let calls = std::cell::Cell::new(0usize);
    let mut rng = seeded_rng_from_u64(11);
    let _ = weighted_index_by_with_rng(
        &items,
        |weight| {
            calls.set(calls.get() + 1);
            *weight
        },
        &mut rng,
    );
    assert!(
        calls.get() > items.len(),
        "projection must run a full total pass plus a selection pass, got {} calls for {} items",
        calls.get(),
        items.len()
    );
    assert!(
        calls.get() <= items.len() * 2,
        "no item may be projected more than twice, got {} calls",
        calls.get()
    );
}

/// GAP: duplicate bytes in a caller alphabet act as a multiset and skew the
/// distribution toward the duplicated byte. The API documents an alphabet,
/// not a set; deduplicating would change emitted-token statistics. Pinned so
/// callers with strict uniformity requirements know to deduplicate first.
#[test]
fn duplicate_alphabet_bytes_bias_output() {
    let alphabet = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaab";
    let mut rng = seeded_rng_from_u64(13);
    let value = guise_choice::random_ascii_string_with_rng(256, alphabet, &mut rng)
        .expect("valid alphabet");
    let bees = value.bytes().filter(|byte| *byte == b'b').count();
    assert!(
        bees < 40,
        "a 31:1 alphabet should yield few `b`s, got {bees}/256"
    );
}

/// GAP: the shared token alphabet is lowercase-plus-digits only, never
/// uppercase or punctuation, because it feeds DNS labels and case-insensitive
/// logs. Adding characters changes the token surface of every consumer.
#[test]
fn lower_alphanumeric_alphabet_stays_dns_safe() {
    assert_eq!(LOWER_ALPHANUMERIC.len(), 36);
    assert!(LOWER_ALPHANUMERIC
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
}
/// GAP: a non-deterministic projection that returns a usable weight on the first
/// pass (total calculation) but an unusable weight (0.0, NaN, or negative) on
/// the second pass (selection pass) fails closed by returning `None`.
#[test]
fn non_deterministic_projection_becoming_unusable_fails_closed() {
    let items = [1.0f64];
    let calls = std::cell::Cell::new(0usize);
    let mut rng = seeded_rng_from_u64(17);
    let result = weighted_index_by_with_rng(
        &items,
        |_| {
            let count = calls.get();
            calls.set(count + 1);
            if count == 0 {
                1.0 // Usable on pass 1 (total)
            } else {
                0.0 // Unusable on pass 2 (selection)
            }
        },
        &mut rng,
    );
    assert_eq!(
        result, None,
        "a projection becoming unusable on selection pass must fail closed returning None"
    );
}

/// GAP: subnormal positive floating point weights (e.g. `f64::MIN_POSITIVE` or
/// `5e-324`) are usable and sampled uniformly without underflow panics.
#[test]
fn subnormal_positive_weights_are_usable() {
    let mut rng = seeded_rng_from_u64(19);
    let subnormal = f64::MIN_POSITIVE;
    let result = weighted_index_with_rng(&[subnormal, subnormal], &mut rng);
    assert!(
        result == Some(0) || result == Some(1),
        "subnormal weights should be usable and return Some index, got {result:?}"
    );
}

/// GAP: invalid caller alphabets (empty or non-ASCII) fail closed returning `None`
/// even when a zero-length string is requested. Alphabet validity is an input contract
/// checked prior to length considerations.
#[test]
fn zero_length_request_with_invalid_alphabet_fails_closed() {
    let mut rng = seeded_rng_from_u64(27);
    assert_eq!(
        guise_choice::random_ascii_string_with_rng(0, b"", &mut rng),
        None,
        "empty alphabet on 0-len request must return None"
    );
    assert_eq!(
        guise_choice::random_ascii_string_with_rng(0, &[0x80], &mut rng),
        None,
        "non-ASCII alphabet on 0-len request must return None"
    );
}

/// GAP: single maximal finite float weight (`f64::MAX`) is finite and usable without
/// total sum overflow.
#[test]
fn single_maximal_finite_weight_is_usable() {
    let mut rng = seeded_rng_from_u64(29);
    assert_eq!(
        weighted_index_with_rng(&[f64::MAX], &mut rng),
        Some(0),
        "single f64::MAX weight should not overflow sum and return Some(0)"
    );
}
