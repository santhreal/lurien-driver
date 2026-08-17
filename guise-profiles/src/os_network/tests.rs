use super::*;
use crate::{profile_user_agent, user_agent_facts, ALL_PROFILES};

#[test]
fn every_persona_has_a_network_stack_with_a_known_initial_ttl() {
    for profile in ALL_PROFILES {
        // `profile_os_network_stack` fails closed (panics) on an Unknown
        // platform rather than silently substituting the Linux stack (TTL 64)
        //: the exact G017 tell. Asserting the precondition explicitly here
        // proves that panic arm dead over the whole catalogue with a targeted
        // message, so a future persona with no modeled platform fails CI
        // before the silent Unix TTL could ship.
        assert_ne!(
            profile_platform(*profile),
            UserAgentPlatform::Unknown,
            "{profile:?} resolves to an Unknown platform; profile_os_network_stack \
                 would fail closed instead of emitting a coherent stack"
        );

        let stack = profile_os_network_stack(*profile);
        assert!(
            KNOWN_INITIAL_TTLS.contains(&stack.initial_ttl),
            "{profile:?} initial TTL {} is not a canonical OS default",
            stack.initial_ttl
        );
        // A persona's modeled TTL must survive a zero-hop de-hop, i.e. the
        // self-probe that defends it can recover the initial value from an
        // observed (hop-decreased) wire TTL. A non-canonical TTL would be
        // unverifiable by that very probe.
        assert_eq!(
            infer_initial_ttl(stack.initial_ttl),
            stack.initial_ttl,
            "{profile:?} initial TTL {} does not survive a zero-hop de-hop",
            stack.initial_ttl
        );
        assert!(stack.df, "{profile:?} modern stack must set the DF bit");
        assert_eq!(stack.tcp_mss, 1460, "{profile:?} 1500-MTU MSS baseline");
        assert!(
            !stack.tcp_options_layout.is_empty(),
            "{profile:?} must carry a TCP options layout"
        );
        // The stack's OS must equal the persona's claimed OS family.
        assert_eq!(stack.os, profile_platform(*profile));
    }
}

#[test]
fn profile_platform_agrees_with_user_agent_string_parser() {
    // Module-pair contract: the pure const fn projection must agree with the
    // string parser the rest of the crate already trusts, for every persona.
    for profile in ALL_PROFILES {
        let from_const = profile_platform(*profile);
        let from_ua = user_agent_facts(profile_user_agent(*profile)).platform;
        assert_eq!(
            from_const, from_ua,
            "{profile:?}: profile_platform disagrees with UA parser"
        );
    }
}

#[test]
fn windows_personas_emit_ttl_128_unix_personas_emit_ttl_64() {
    // The load-bearing L2 discriminator across the rotation set.
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxWindows).initial_ttl,
        128
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::ChromeWindowsStable).initial_ttl,
        128
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::EdgeWindowsStable).initial_ttl,
        128
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxLinux).initial_ttl,
        64
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::ChromeMacStable).initial_ttl,
        64
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::SafariIphone).initial_ttl,
        64
    );
    assert_eq!(
        profile_os_network_stack(StealthProfile::ChromeAndroid).initial_ttl,
        64
    );
}

#[test]
fn infer_initial_ttl_dehops_each_band() {
    // Unix band: a Linux host 8 hops away still de-hops to 64.
    assert_eq!(infer_initial_ttl(64), 64);
    assert_eq!(infer_initial_ttl(56), 64);
    assert_eq!(infer_initial_ttl(1), 64);
    // Windows band.
    assert_eq!(infer_initial_ttl(128), 128);
    assert_eq!(infer_initial_ttl(120), 128);
    assert_eq!(infer_initial_ttl(65), 128);
    // Legacy band.
    assert_eq!(infer_initial_ttl(255), 255);
    assert_eq!(infer_initial_ttl(200), 255);
    assert_eq!(infer_initial_ttl(129), 255);
    // Unmeasurable.
    assert_eq!(infer_initial_ttl(0), 0);
}

#[test]
fn boundary_ttls_round_into_the_lower_band() {
    // Exactly-on-boundary observed TTLs belong to the OS that emits them.
    assert_eq!(infer_initial_ttl(64), 64);
    assert_eq!(infer_initial_ttl(128), 128);
    // One above a boundary rolls into the next band.
    assert_eq!(infer_initial_ttl(64 + 1), 128);
    assert_eq!(infer_initial_ttl(128 + 1), 255);
}

#[test]
fn windows_persona_on_a_linux_host_is_flagged_incoherent() {
    // A FirefoxWindows persona whose packets arrive with a de-hopped TTL of
    // 64 is the exact G017 tell: claims Windows (128), wire says Unix (64).
    let verdict = os_network_coherence(StealthProfile::FirefoxWindows, 54);
    assert!(!verdict.is_coherent());
    match verdict {
        NetworkOsCoherence::Mismatch {
            expected_os,
            expected_ttl,
            observed_initial_ttl,
        } => {
            assert_eq!(expected_os, UserAgentPlatform::Windows);
            assert_eq!(expected_ttl, 128);
            assert_eq!(observed_initial_ttl, 64);
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

#[test]
fn linux_persona_on_a_linux_host_is_coherent() {
    let verdict = os_network_coherence(StealthProfile::FirefoxLinux, 54);
    assert!(verdict.is_coherent());
    match verdict {
        NetworkOsCoherence::Coherent { os, initial_ttl } => {
            assert_eq!(os, UserAgentPlatform::Linux);
            assert_eq!(initial_ttl, 64);
        }
        other => panic!("expected Coherent, got {other:?}"),
    }
}

#[test]
fn windows_persona_on_a_windows_host_is_coherent() {
    // TTL 122 de-hops to 128 → coherent for a Windows persona.
    let verdict = os_network_coherence(StealthProfile::ChromeWindowsStable, 122);
    assert!(verdict.is_coherent());
}

#[test]
fn unmeasurable_ttl_is_unknown_not_a_false_mismatch() {
    assert_eq!(
        os_network_coherence(StealthProfile::FirefoxLinux, 0),
        NetworkOsCoherence::Unknown
    );
}

#[test]
fn options_layout_distinguishes_os_families() {
    // Linux and Windows have distinct, non-overlapping SYN option orders.
    let linux = profile_os_network_stack(StealthProfile::FirefoxLinux).tcp_options_layout;
    let windows = profile_os_network_stack(StealthProfile::FirefoxWindows).tcp_options_layout;
    let macos = profile_os_network_stack(StealthProfile::SafariMacStable).tcp_options_layout;
    assert_ne!(linux, windows);
    assert_ne!(linux, macos);
    assert_ne!(windows, macos);

    assert!(os_network_options_match(
        StealthProfile::FirefoxLinux,
        "mss,sok,ts,nop,ws"
    ));
    // A Windows persona sending a Linux option layout is not a match.
    assert!(!os_network_options_match(
        StealthProfile::FirefoxWindows,
        "mss,sok,ts,nop,ws"
    ));
}

#[test]
fn darwin_family_shares_one_stack() {
    // macOS and iOS run the same XNU network stack; their fingerprints match.
    let mac = profile_os_network_stack(StealthProfile::SafariMacStable);
    let iphone = profile_os_network_stack(StealthProfile::SafariIphone);
    assert_eq!(mac.initial_ttl, iphone.initial_ttl);
    assert_eq!(mac.tcp_options_layout, iphone.tcp_options_layout);
    assert_eq!(mac.tcp_window, iphone.tcp_window);
    assert_eq!(mac.tcp_window_scale, iphone.tcp_window_scale);
    // ...but they are still distinct OS families in the taxonomy.
    assert_ne!(mac.os, iphone.os);
}

#[test]
fn unknown_platform_has_no_stack() {
    assert_eq!(os_network_stack(UserAgentPlatform::Unknown), None);
    assert!(os_network_stack(UserAgentPlatform::Windows).is_some());
}

#[test]
fn p0f_signature_renders_every_field_faithfully() {
    // Linux: autotuned window → `*`, full olayout, DF set.
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxLinux).p0f_signature(),
        "64:1460:*,7:mss,sok,ts,nop,ws:df"
    );
    // Windows: fixed 64240 window, Windows olayout, TTL 128.
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxWindows).p0f_signature(),
        "128:1460:64240,8:mss,nop,ws,nop,nop,sok:df"
    );
    // macOS: Darwin olayout with the eol terminator, fixed 65535 window.
    assert_eq!(
        profile_os_network_stack(StealthProfile::SafariMacStable).p0f_signature(),
        "64:1460:65535,6:mss,nop,ws,nop,nop,ts,sok,eol:df"
    );
}

#[test]
fn p0f_signatures_separate_the_os_families() {
    // Each OS family's signature must be distinct, that's the whole point of
    // a fingerprint. (Linux vs Android share TTL+olayout but differ in wscale.)
    let sigs = [
        StealthProfile::FirefoxLinux,
        StealthProfile::FirefoxWindows,
        StealthProfile::SafariMacStable,
        StealthProfile::ChromeAndroid,
    ]
    .map(|p| profile_os_network_stack(p).p0f_signature());
    for (i, a) in sigs.iter().enumerate() {
        for b in &sigs[i + 1..] {
            assert_ne!(a, b, "two OS families share a p0f signature: {a}");
        }
    }
}

#[test]
fn fixed_windows_window_is_the_modern_default() {
    match profile_os_network_stack(StealthProfile::ChromeWindowsStable).tcp_window {
        TcpWindow::Fixed(w) => assert_eq!(w, 64240),
        TcpWindow::MssScaled => panic!("Windows advertises a fixed SYN window"),
    }
    // Linux autotunes, so its window is not a constant.
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxLinux).tcp_window,
        TcpWindow::MssScaled
    );
}

#[test]
fn ja4t_matches_foxio_windows11_reference() {
    // FoxIO's published Windows-11 JA4T reference is `64240_2-1-3-1-1-4_1460_8`.
    // Our independently-modeled Windows stack (Fixed(64240) window, olayout
    // `mss,nop,ws,nop,nop,sok` → kinds 2-1-3-1-1-4, MSS 1460, wscale 8) must
    // reproduce it byte-for-byte, the validation anchor that proves the
    // option-kind mapping and field order are the real JA4+ algorithm, not a
    // home-grown lookalike.
    for profile in [
        StealthProfile::FirefoxWindows,
        StealthProfile::ChromeWindowsStable,
        StealthProfile::EdgeWindowsStable,
    ] {
        assert_eq!(
            profile_os_network_stack(profile).ja4t().unwrap(),
            "64240_2-1-3-1-1-4_1460_8",
            "{profile:?} JA4T diverged from the FoxIO Windows-11 reference"
        );
    }
}

#[test]
fn ja4t_autotuned_window_renders_as_wildcard_not_a_fabricated_value() {
    // Linux/Android autotune the receive window; JA4T's window field is `*`
    // (non-asserting) while the option/MSS/wscale tail stays concrete. Inventing
    // a window number here would be a fabricated fingerprint.
    assert_eq!(
        profile_os_network_stack(StealthProfile::FirefoxLinux)
            .ja4t()
            .unwrap(),
        "*_2-4-8-1-3_1460_7"
    );
    // Android shares Linux's olayout but scales the window by 8, not 7, so its
    // JA4T tail is distinct even though both wildcard the window.
    assert_eq!(
        profile_os_network_stack(StealthProfile::ChromeAndroid)
            .ja4t()
            .unwrap(),
        "*_2-4-8-1-3_1460_8"
    );
    assert_ne!(
        profile_os_network_stack(StealthProfile::FirefoxLinux)
            .ja4t()
            .unwrap(),
        profile_os_network_stack(StealthProfile::ChromeAndroid)
            .ja4t()
            .unwrap(),
    );
}

#[test]
fn ja4t_macos_renders_the_full_darwin_option_chain() {
    // macOS olayout `mss,nop,ws,nop,nop,ts,sok,eol` → 2-1-3-1-1-8-4-0, fixed
    // 65535 window, wscale 6.
    assert_eq!(
        profile_os_network_stack(StealthProfile::SafariMacStable)
            .ja4t()
            .unwrap(),
        "65535_2-1-3-1-1-8-4-0_1460_6"
    );
}

#[test]
fn ja4t_darwin_family_shares_one_fingerprint() {
    // macOS and iOS run the same XNU stack, so their JA4T must be identical
    // the TCP analogue of `darwin_family_shares_one_stack`.
    assert_eq!(
        profile_os_network_stack(StealthProfile::SafariMacStable)
            .ja4t()
            .unwrap(),
        profile_os_network_stack(StealthProfile::SafariIphone)
            .ja4t()
            .unwrap(),
    );
}

#[test]
fn every_persona_renders_a_ja4t_with_only_known_option_kinds() {
    // The fail-closed arm of `ja4t` must be dead over the shipped catalogue:
    // every persona's olayout maps cleanly to IANA option kinds. A future
    // persona with an unmapped token fails CI here before a wrong JA4T ships.
    for profile in ALL_PROFILES {
        let ja4t = profile_os_network_stack(*profile)
            .ja4t()
            .unwrap_or_else(|e| panic!("{profile:?} JA4T failed closed: {e}"));
        // Shape: four underscore-separated fields, the 2nd a hyphen-joined kind list.
        let fields: Vec<&str> = ja4t.split('_').collect();
        assert_eq!(fields.len(), 4, "{profile:?} JA4T {ja4t} is not 4 fields");
        assert!(
            fields[1].split('-').all(|k| k.parse::<u8>().is_ok()),
            "{profile:?} JA4T option field {} is not all numeric kinds",
            fields[1]
        );
    }
}

#[test]
fn ja4t_fails_closed_on_an_unmapped_option_token() {
    // Law 10: an options layout with a token outside the IANA registry must
    // surface loudly, never silently drop the option into a shorter (wrong)
    // fingerprint.
    let bogus = OsNetworkStack {
        os: UserAgentPlatform::Linux,
        initial_ttl: 64,
        tcp_mss: 1460,
        tcp_window_scale: 7,
        tcp_window: TcpWindow::MssScaled,
        tcp_options_layout: "mss,frobnicate,ws",
        df: true,
    };
    match bogus.ja4t() {
        Err(Ja4tError { unknown_option }) => assert_eq!(unknown_option, "frobnicate"),
        other => panic!("expected fail-closed on unknown option, got {other:?}"),
    }
}

#[test]
fn ja4t_matches_observed_fixed_window_requires_exact_window() {
    // Windows advertises a fixed SYN window, so the observed JA4T must match in
    // full (including the window field (to be coherent)).
    let win = profile_os_network_stack(StealthProfile::FirefoxWindows);
    assert!(win.ja4t_matches_observed("64240_2-1-3-1-1-4_1460_8"));
    // A different (even plausible) window on a fixed-window OS is a real tell.
    assert!(!win.ja4t_matches_observed("65535_2-1-3-1-1-4_1460_8"));
}

#[test]
fn ja4t_matches_observed_autotuned_window_is_wildcarded() {
    // Linux autotunes the receive window, so a concrete observed window must NOT
    // trigger a mismatch as long as the OS-discriminating option/MSS tail agrees.
    let linux = profile_os_network_stack(StealthProfile::FirefoxLinux);
    assert!(linux.ja4t_matches_observed("29200_2-4-8-1-3_1460_7"));
    assert!(linux.ja4t_matches_observed("64240_2-4-8-1-3_1460_7"));
    // But the OS-discriminating tail (here a Windows-shaped option layout) still
    // has to match (a Linux persona emitting a Windows TCP tail is the G017 tell).
    assert!(!linux.ja4t_matches_observed("29200_2-1-3-1-1-4_1460_8"));
}

#[test]
fn ja4t_matches_observed_treats_window_scale_as_host_variable_advisory() {
    // REAL-WIRE FINDING (2026-06-13): the egress SYN captured from this very Linux
    // host via tcpdump is `win 64240, options [mss,sackOK,TS,nop,wscale 10]` →
    // JA4T `64240_2-4-8-1-3_1460_10`. The host advertises wscale 10 (large
    // net.ipv4.tcp_rmem) where the modeled stock Linux is wscale 7. The window-scale
    // is a per-host kernel tunable, NOT an OS-family constant, so a coherence
    // self-probe must treat it as advisory, else a legitimate, fully-coherent Linux
    // persona on a tuned Linux host would be FALSE-FLAGGED incoherent. The option
    // layout `2-4-8-1-3` + MSS 1460 (the real OS discriminators) match, so this IS
    // coherent.
    let linux = profile_os_network_stack(StealthProfile::FirefoxLinux);
    assert!(
        linux.ja4t_matches_observed("64240_2-4-8-1-3_1460_10"),
        "the real captured tuned-Linux SYN (wscale 10) must be coherent with the Linux stack"
    );
    // A wscale drift alone (7→8, the Android wscale) is therefore NOT a mismatch:
    // Linux and Android ship the same kernel TCP stack (identical `2-4-8-1-3`
    // layout + MSS) and are not separable at the TCP layer by wscale, exactly as
    // the Darwin macOS↔iOS pair is already JA4T-identical in this model. The
    // mobile/desktop tell lives in the UA/client-hints/screen layers, not here.
    assert!(linux.ja4t_matches_observed("29200_2-4-8-1-3_1460_8"));
    // The option LAYOUT remains the hard discriminator: a Darwin/Windows-shaped
    // layout is still caught regardless of wscale.
    assert!(!linux.ja4t_matches_observed("29200_2-1-3-1-1-8-4-0_1460_6"));
    assert!(!linux.ja4t_matches_observed("29200_2-1-3-1-1-4_1460_10"));
}

#[test]
fn ja4t_matches_observed_fails_closed_on_malformed_observation() {
    // A non-4-field observed string is never read as agreement (Law 10).
    let linux = profile_os_network_stack(StealthProfile::FirefoxLinux);
    assert!(!linux.ja4t_matches_observed(""));
    assert!(!linux.ja4t_matches_observed("64240_2-4-8-1-3_1460")); // 3 fields
    assert!(!linux.ja4t_matches_observed("not a ja4t at all"));
}

/// Locks the BACKLOG one-place fix: `p0f_signature` and `ja4t` must render the
/// SAME window token for the same stack, because both delegate to the single
/// `window_field` owner. A drift between them would give the p0f self-probe
/// and the JA4T wire comparison different expectations for one SYN.
#[test]
fn p0f_and_ja4t_render_the_same_window_token() {
    for profile in ALL_PROFILES {
        let stack = profile_os_network_stack(*profile);
        let expected_window = match stack.tcp_window {
            TcpWindow::MssScaled => "*".to_string(),
            TcpWindow::Fixed(value) => value.to_string(),
        };
        let p0f_window = stack
            .p0f_signature()
            .split(':')
            .nth(2)
            .expect("p0f signature has a window field")
            .split(',')
            .next()
            .expect("p0f window field")
            .to_string();
        let ja4t_window = stack
            .ja4t()
            .expect("catalogue stacks render JA4T")
            .split('_')
            .next()
            .expect("ja4t has a window field")
            .to_string();
        assert_eq!(p0f_window, expected_window, "{profile:?} p0f window");
        assert_eq!(ja4t_window, expected_window, "{profile:?} ja4t window");
    }
}
#[test]
fn os_network_options_match_normalizes_whitespace() {
    assert!(os_network_options_match(
        StealthProfile::ChromeWindowsStable,
        "mss, nop, ws, nop, nop, sok"
    ));
    assert!(os_network_options_match(
        StealthProfile::FirefoxLinux,
        " mss , sok , ts , nop , ws "
    ));
    assert!(!os_network_options_match(
        StealthProfile::ChromeWindowsStable,
        "mss, sok, ts, nop, ws"
    ));
}
