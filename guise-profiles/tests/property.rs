//! Property-based and exhaustive-catalog tests for guise-profiles.
//!
//! The crate is pure reference data, so the invariants that matter are
//! catalogue-wide: lookups round-trip, modulo accessors stay in range, and
//! TTL inference is monotonic and total over the whole `u8` domain.

use guise_profiles::{
    infer_initial_ttl, named_profile, os_network_coherence, profile_hardware_at,
    profile_hardware_variants, profile_name, rotate, ALL_PROFILES, KNOWN_INITIAL_TTLS,
};
use proptest::prelude::*;

/// Every canonical profile resolves back through `named_profile` by its own
/// `profile_name`. A profile whose name did not round-trip would be
/// unreachable from config files.
#[test]
fn every_profile_name_round_trips_through_named_profile() {
    for profile in ALL_PROFILES {
        assert_eq!(
            named_profile(profile_name(*profile)),
            Some(*profile),
            "{profile:?} does not round-trip through named_profile"
        );
    }
}

proptest! {
    /// `rotate` is total over `usize`: any index, including `usize::MAX`,
    /// yields a catalog profile instead of panicking.
    #[test]
    fn rotate_is_total_for_any_index(index in any::<usize>()) {
        let profile = rotate(index);
        prop_assert!(guise_profiles::PROFILES.iter().any(|entry| entry.name == profile.name),
            "rotate yielded {:?} outside PROFILES", profile.name);
    }

    /// `profile_hardware_at` always returns a member of the profile's own
    /// hardware table, for any index (modulo selection, never out of range).
    #[test]
    fn hardware_at_always_returns_a_table_member(
        profile_index in 0..ALL_PROFILES.len(),
        index in any::<usize>(),
    ) {
        let profile = ALL_PROFILES[profile_index];
        let variants = profile_hardware_variants(profile);
        let hardware = profile_hardware_at(profile, index);
        prop_assert!(variants.contains(&hardware),
            "{profile:?} index {index} escaped its hardware table");
    }

    /// `infer_initial_ttl` over the whole observed-TTL domain: zero stays
    /// zero (unverifiable, never guessed), every positive observation maps to
    /// a canonical initial TTL that is >= the observation (hops only ever
    /// decrease TTL).
    #[test]
    fn infer_initial_ttl_is_canonical_and_monotone(observed in any::<u8>()) {
        let inferred = infer_initial_ttl(observed);
        if observed == 0 {
            prop_assert_eq!(inferred, 0);
        } else {
            prop_assert!(KNOWN_INITIAL_TTLS.contains(&inferred),
                "observed {observed} inferred non-canonical {inferred}");
            prop_assert!(inferred >= observed,
                "observed {observed} cannot exceed initial {inferred}");
        }
    }

    /// A zero observed TTL is `Unknown` for every persona: the coherence
    /// probe must never fabricate a verdict without a wire value.
    #[test]
    fn coherence_with_zero_ttl_is_unknown_for_every_profile(
        profile_index in 0..ALL_PROFILES.len(),
    ) {
        use guise_profiles::NetworkOsCoherence;
        let profile = ALL_PROFILES[profile_index];
        prop_assert!(matches!(
            os_network_coherence(profile, 0),
            NetworkOsCoherence::Unknown
        ));
    }

    /// `named_profile` never panics on arbitrary input, and lookups are
    /// case- and whitespace-insensitive by contract.
    #[test]
    fn named_profile_is_robust_and_normalized(name in ".*") {
        let direct = named_profile(&name);
        let noised = named_profile(&format!("  {}  ", name.to_uppercase()));
        prop_assert_eq!(direct, noised);
    }
}
