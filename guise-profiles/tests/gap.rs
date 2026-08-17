//! Gap tests: deliberate behaviors and documented limitations of
//! guise-profiles that must stay pinned so any future change is a conscious
//! decision, not silent drift.

use guise_profiles::{
    infer_initial_ttl, os_network_coherence, profile_os_network_stack, NetworkOsCoherence,
    StealthProfile, ALL_PROFILES, DEFAULT_STEALTH_PROFILE,
};

/// GAP: `infer_initial_ttl(0)` is `0`, not a guess. A zero TTL on the wire is
/// unverifiable (the packet should not exist), so the de-hop probe refuses to
/// invent an initial value. This is deliberate fail-closed behavior; mapping
/// zero to the Unix default 64 would fabricate evidence for the coherence
/// gate. Pinned so "helpfully" defaulting it is a reviewed change.
#[test]
fn zero_observed_ttl_never_guesses_an_initial_ttl() {
    assert_eq!(infer_initial_ttl(0), 0);
    for profile in ALL_PROFILES {
        assert!(matches!(
            os_network_coherence(*profile, 0),
            NetworkOsCoherence::Unknown
        ));
    }
}

/// GAP: `profile_os_network_stack` PANICS on an Unknown platform instead of
/// substituting the Linux stack (TTL 64). This is intentional fail-closed
/// design (the G017 TTL tell: a fabricated Linux stack for an unmodeled OS
/// is worse than a crash) and is documented as NOT a finding in the family
/// backlog. Pinned so softening it to a silent default is impossible without
/// editing this test. No canonical persona may hit that arm.
#[test]
fn unknown_platform_stack_fails_closed_and_catalogue_avoids_it() {
    use guise_profiles::{os_network_stack, UserAgentPlatform};
    assert!(os_network_stack(UserAgentPlatform::Unknown).is_none());
    for profile in ALL_PROFILES {
        // Never panics over the canonical catalogue.
        let _ = profile_os_network_stack(*profile);
    }
}

/// GAP: the legacy IE11 persona keeps `LEGACY_ACCEPT_ENCODING`
/// ("gzip, deflate", no Brotli) while every other persona sends the modern
/// default. This asymmetry is deliberate reference data: an IE11 sending
/// `br` would be a catalogue drift tell. Pinned per-persona so "unifying"
/// the encodings is a deliberate data change.
#[test]
fn ie11_keeps_legacy_accept_encoding() {
    use guise_profiles::{profile_facts, DEFAULT_ACCEPT_ENCODING, LEGACY_ACCEPT_ENCODING};
    for profile in ALL_PROFILES {
        let expected = if matches!(profile, StealthProfile::Ie11Windows) {
            LEGACY_ACCEPT_ENCODING
        } else {
            DEFAULT_ACCEPT_ENCODING
        };
        assert_eq!(
            profile_facts(*profile).accept_encoding,
            expected,
            "{profile:?}"
        );
    }
}

/// GAP: the fleet-wide default persona is Chrome on Windows
/// (`DEFAULT_STEALTH_PROFILE`), not a rotation or a random pick. Several
/// launch paths rely on this being the most common real-browser identity;
/// changing the default changes the fingerprint of every consumer that did
/// not pin a persona. Pinned so the change is always deliberate.
#[test]
fn default_persona_stays_chrome_windows() {
    assert!(matches!(
        DEFAULT_STEALTH_PROFILE,
        StealthProfile::ChromeWindowsStable
    ));
}
