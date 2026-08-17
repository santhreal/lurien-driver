//! Network OS-stack fingerprint projection for a [`StealthProfile`].
//!
//! Every persona in this crate already carries its browser-layer identity
//! (User-Agent, headers, hardware, client hints). This module adds the missing
//! **transport-layer** projection: the TCP/IP SYN fingerprint the persona's
//! *claimed operating system* emits on the wire (initial IP TTL, TCP options
//! layout, window scale, MSS, advertised window).
//!
//! Why it lives here, not in the runtime crate: this is pure, OS-determined
//! reference data, the same provenance class as the canonical User-Agent
//! strings and client-hint brands already in [`crate`]. The *measurement* of a
//! live socket's TTL is runtime/IO work and belongs in the consuming stealth
//! crate; the reference table and the coherence predicate are pure and belong
//! beside the rest of the persona's identity facts so one source of truth backs
//! both the browser layer and the network layer.
//!
//! Provenance: the per-OS constants are the canonical modern-default SYN
//! signatures catalogued by p0f3 (`p0f.fp`) and RFC 9293 §3.1. The
//! **initial TTL** is the load-bearing, assertable coherence key, it takes one
//! of three OS-family values (64 for Unix-family kernels, 128 for Windows NT,
//! 255 for legacy network stacks) and is recoverable from an observed TTL by
//! rounding up past the (strictly decreasing) router-hop count. Window scale and
//! advertised window vary per connection and per kernel tunable, so they are
//! recorded for the transport layer's use (e.g. coherent packet rewriting) but
//! are deliberately **not** asserted as ground truth about any live host.

use crate::{profile_platform, StealthProfile, UserAgentPlatform};

/// The TCP advertised window a kernel sends on the SYN, by OS family.
///
/// Linux and Android autotune the receive window and advertise it as a varying
/// multiple of MSS, so no single value is assertable. Windows, macOS, and iOS
/// send a fixed initial window on the SYN before scaling engages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpWindow {
    /// Kernel-autotuned receive window, advertised as a varying multiple of MSS.
    /// The per-connection value is not a constant and must not be asserted.
    MssScaled,
    /// Fixed advertised window (in bytes) the OS sends on the SYN.
    Fixed(u16),
}

/// The TCP/IP SYN fingerprint a persona's claimed OS emits on the wire.
///
/// One descriptor per OS family, the TCP/IP stack is determined by the kernel,
/// not the browser, so all Windows personas share a stack, all Linux personas
/// share a stack, and so on. Resolve it for a persona with
/// [`profile_os_network_stack`] or for a bare OS family with
/// [`os_network_stack`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsNetworkStack {
    /// OS family this stack belongs to.
    pub os: UserAgentPlatform,
    /// Initial IP TTL the OS stamps on outbound packets. The assertable
    /// coherence key: 64 (Unix-family), 128 (Windows NT), or 255 (legacy).
    pub initial_ttl: u8,
    /// TCP Maximum Segment Size advertised on a 1500-byte-MTU link.
    pub tcp_mss: u16,
    /// TCP window-scale shift exponent typical for this OS family (advisory).
    pub tcp_window_scale: u8,
    /// Advertised receive window on the SYN.
    pub tcp_window: TcpWindow,
    /// p0f `olayout`: the order of TCP options on the SYN, a strong per-OS
    /// discriminator (e.g. Linux `mss,sok,ts,nop,ws` vs Windows
    /// `mss,nop,ws,nop,nop,sok`).
    pub tcp_options_layout: &'static str,
    /// Whether the IP "Don't Fragment" bit is set (true for all modern stacks).
    pub df: bool,
}

impl OsNetworkStack {
    /// Render the TCP advertised-window field shared by [`Self::p0f_signature`]
    /// and [`Self::ja4t`]: `*` for the kernel-autotuned ([`TcpWindow::MssScaled`])
    /// case, deliberately NOT inventing an `mss*N` multiple we cannot assert,
    /// and the literal value when the OS sends a fixed window. Single owner of
    /// the `*`/fixed convention so the two signatures cannot drift.
    fn window_field(&self) -> String {
        match self.tcp_window {
            TcpWindow::MssScaled => "*".to_string(),
            TcpWindow::Fixed(value) => value.to_string(),
        }
    }

    /// Render the persona's expected SYN signature in a faithful p0f-style form:
    /// `ittl:mss:window,wscale:olayout:quirks`.
    ///
    /// This is the caller-facing string a p0f-class self-probe (G021) compares
    /// the egress against. It consumes every field of the stack. The advertised
    /// window follows the [`Self::window_field`] convention.
    ///
    /// Example (Linux): `64:1460:*,7:mss,sok,ts,nop,ws:df`.
    #[must_use]
    pub fn p0f_signature(&self) -> String {
        let window = self.window_field();
        let quirks = if self.df { "df" } else { "" };
        format!(
            "{}:{}:{},{}:{}:{}",
            self.initial_ttl,
            self.tcp_mss,
            window,
            self.tcp_window_scale,
            self.tcp_options_layout,
            quirks
        )
    }

    /// Compute the persona's expected **JA4T**: FoxIO's JA4+ TCP-client
    /// fingerprint, from its SYN parameters:
    /// `window_size _ option-kinds(hyphen-joined) _ MSS _ window-scale`,
    /// where the option kinds are the IANA TCP option numbers in the exact SYN
    /// order ([`Self::tcp_options_layout`] mnemonics mapped to their registry
    /// number: EOL=0, NOP=1, MSS=2, WScale=3, SACK-permitted=4, SACK=5,
    /// Timestamps=8).
    ///
    /// Validated byte-for-byte against FoxIO's published Windows-11 reference
    /// `64240_2-1-3-1-1-4_1460_8` (`ja4t_matches_foxio_windows11_reference`).
    /// Unlike [`Self::p0f_signature`] (our own descriptive form), JA4T is the
    /// fingerprint string modern detectors actually compute off the wire, so an
    /// observed-vs-expected comparison can use it directly.
    ///
    /// The window component is the literal advertised window for a fixed-window
    /// OS (Windows/macOS/iOS); for a kernel-autotuned receive window
    /// ([`TcpWindow::MssScaled`], Linux/Android) it renders as `*`: the same
    /// non-asserting marker `p0f_signature` uses, because the per-connection
    /// value is not a constant and inventing one would be a fabricated
    /// fingerprint. A consumer treats `*` as a wildcard on the window field only;
    /// the option/MSS/wscale tail stays fully concrete and assertable.
    ///
    /// # Errors
    ///
    /// Fails closed with [`Ja4tError`] if the options layout carries a token with
    /// no canonical IANA option-kind, the unmapped option is surfaced rather
    /// than silently dropped (Law 10), so a malformed or future-extended layout
    /// can never yield a quietly-wrong JA4T.
    pub fn ja4t(&self) -> Result<String, Ja4tError> {
        let window = self.window_field();
        let mut kinds = Vec::new();
        for option in self.tcp_options_layout.split(',') {
            let token = option.trim();
            match tcp_option_kind(token) {
                Some(kind) => kinds.push(kind.to_string()),
                None => {
                    return Err(Ja4tError {
                        unknown_option: token.to_string(),
                    })
                }
            }
        }
        Ok(format!(
            "{}_{}_{}_{}",
            window,
            kinds.join("-"),
            self.tcp_mss,
            self.tcp_window_scale
        ))
    }

    /// Whether an observed JA4T, computed from a real SYN seen on the wire, is
    /// coherent with this stack's expected JA4T, for the **persona self-coherence**
    /// verdict (does my real egress betray the OS my persona claims?).
    ///
    /// Two fields are the hard OS-family discriminators and MUST match exactly:
    /// - the **option-kind layout** (field 1), e.g. Linux/Android `2-4-8-1-3` vs
    ///   Windows `2-1-3-1-1-4` vs Darwin `2-1-3-1-1-8-4-0`: the strong tell, and
    /// - the **MSS** (field 2).
    ///
    /// The **window** (field 0) is a wildcard for autotuned-window OS families
    /// ([`TcpWindow::MssScaled`], where [`Self::ja4t`] renders `*`): a real Linux
    /// SYN advertises a concrete autotuned window (e.g. `29200`) that varies per
    /// connection, so asserting it would produce false mismatches. For a
    /// fixed-window OS (Windows/macOS/iOS) the window must match exactly.
    ///
    /// The **window-scale** (field 3) is deliberately NOT asserted here. It is a
    /// per-host kernel tunable, not an OS-family constant: on Linux it is derived
    /// from `net.ipv4.tcp_rmem` (stock ≈ 7, but a tuned host advertises 8/10/…;
    /// measured `wscale 10` on a large-`tcp_rmem` host vs the modeled stock 7). The
    /// field's own model doc already marks it *advisory*. Asserting it would
    /// FALSE-POSITIVE flag a legitimate, fully-coherent persona on a tuned host as
    /// "incoherent", a soundness failure for a coherence verdict (a false
    /// accusation). The option layout already carries the OS-family signal a
    /// detector classifies on, so dropping the wscale assertion loses no real
    /// discrimination: the only stacks distinguished by wscale alone are
    /// Linux↔Android (`2-4-8-1-3`, wscale 7 vs 8), which, like the already
    /// JA4T-identical macOS↔iOS Darwin pair, are genuinely not separable at the
    /// TCP layer (the mobile/desktop tell lives in the UA/client-hints/screen
    /// layers guise checks separately). `wscale` is still rendered by [`Self::ja4t`]
    /// (the detector-facing predicted fingerprint), it is only this *coherence
    /// matcher* that treats it as advisory.
    ///
    /// Fails closed: an observed string that is not a 4-field JA4T, or an
    /// expected JA4T that cannot render, returns `false`: an unparseable
    /// observation is never read as agreement (Law 10).
    #[must_use]
    pub fn ja4t_matches_observed(&self, observed_ja4t: &str) -> bool {
        let Ok(expected) = self.ja4t() else {
            return false;
        };
        let exp: Vec<&str> = expected.split('_').collect();
        let obs: Vec<&str> = observed_ja4t.split('_').collect();
        if exp.len() != 4 || obs.len() != 4 {
            return false;
        }
        // Window (field 0) is a wildcard exactly when we render it as `*`
        // (autotuned receive window); otherwise it must match. The OS-discriminating
        // option layout (1) + MSS (2) are asserted; the host-variable window-scale
        // (3) is advisory (see the doc above for why asserting it is unsound).
        let window_ok = exp[0] == "*" || exp[0] == obs[0];
        window_ok && exp[1] == obs[1] && exp[2] == obs[2]
    }
}

/// Error from [`OsNetworkStack::ja4t`]: a TCP options-layout mnemonic had no
/// canonical IANA option-kind, so a faithful JA4T cannot be rendered. Surfaced
/// loudly rather than dropping the option (Law 10 (no silent fallback)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ja4tError {
    /// The options-layout token with no canonical option-kind mapping.
    pub unknown_option: String,
}

impl core::fmt::Display for Ja4tError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "TCP options token {:?} has no canonical IANA option-kind; refusing to \
             render a JA4T that silently drops it",
            self.unknown_option
        )
    }
}

impl std::error::Error for Ja4tError {}

/// IANA TCP option-kind number for a p0f-style option mnemonic used in
/// [`OsNetworkStack::tcp_options_layout`]. Canonical registry: EOL=0, NOP=1,
/// MSS=2, WScale=3, SACK-permitted=4, SACK=5, Timestamps=8. Returns `None` for
/// any token outside the registry so [`OsNetworkStack::ja4t`] can fail closed.
fn tcp_option_kind(name: &str) -> Option<u8> {
    match name {
        "eol" => Some(0),
        "nop" => Some(1),
        "mss" => Some(2),
        "ws" => Some(3),
        "sok" => Some(4),
        "sack" => Some(5),
        "ts" => Some(8),
        _ => None,
    }
}

const LINUX_STACK: OsNetworkStack = OsNetworkStack {
    os: UserAgentPlatform::Linux,
    initial_ttl: 64,
    tcp_mss: 1460,
    tcp_window_scale: 7,
    tcp_window: TcpWindow::MssScaled,
    tcp_options_layout: "mss,sok,ts,nop,ws",
    df: true,
};

const ANDROID_STACK: OsNetworkStack = OsNetworkStack {
    os: UserAgentPlatform::Android,
    initial_ttl: 64,
    tcp_mss: 1460,
    tcp_window_scale: 8,
    tcp_window: TcpWindow::MssScaled,
    // Android ships the mainline Linux TCP stack, so the options layout matches.
    tcp_options_layout: "mss,sok,ts,nop,ws",
    df: true,
};

const MACOS_STACK: OsNetworkStack = OsNetworkStack {
    os: UserAgentPlatform::MacOs,
    initial_ttl: 64,
    tcp_mss: 1460,
    tcp_window_scale: 6,
    tcp_window: TcpWindow::Fixed(65535),
    tcp_options_layout: "mss,nop,ws,nop,nop,ts,sok,eol",
    df: true,
};

const IOS_STACK: OsNetworkStack = OsNetworkStack {
    os: UserAgentPlatform::Ios,
    initial_ttl: 64,
    tcp_mss: 1460,
    tcp_window_scale: 6,
    // iOS shares the Darwin/XNU network stack with macOS.
    tcp_window: TcpWindow::Fixed(65535),
    tcp_options_layout: "mss,nop,ws,nop,nop,ts,sok,eol",
    df: true,
};

const WINDOWS_STACK: OsNetworkStack = OsNetworkStack {
    os: UserAgentPlatform::Windows,
    initial_ttl: 128,
    tcp_mss: 1460,
    tcp_window_scale: 8,
    tcp_window: TcpWindow::Fixed(64240),
    tcp_options_layout: "mss,nop,ws,nop,nop,sok",
    df: true,
};

/// The canonical initial-TTL values, ascending. An observed TTL de-hops to the
/// smallest entry that is `>=` it (hops only ever decrease TTL).
pub const KNOWN_INITIAL_TTLS: [u8; 3] = [64, 128, 255];

/// Network OS-stack fingerprint for an OS family.
///
/// Returns `None` only for [`UserAgentPlatform::Unknown`]; every concrete OS
/// family has a stack.
#[must_use]
pub const fn os_network_stack(platform: UserAgentPlatform) -> Option<OsNetworkStack> {
    match platform {
        UserAgentPlatform::Linux => Some(LINUX_STACK),
        UserAgentPlatform::Android => Some(ANDROID_STACK),
        UserAgentPlatform::MacOs => Some(MACOS_STACK),
        UserAgentPlatform::Ios => Some(IOS_STACK),
        UserAgentPlatform::Windows => Some(WINDOWS_STACK),
        UserAgentPlatform::Unknown => None,
    }
}

/// Network OS-stack fingerprint for a persona's claimed OS.
///
/// Total: every [`StealthProfile`] maps to a concrete OS family via
/// [`profile_platform`], so a stack always exists. The
/// `every_persona_has_a_concrete_network_stack` test proves this exhaustively
/// over [`ALL_PROFILES`].
///
/// # Panics
///
/// Fails closed (Law 10, no silent fallback) if a persona ever resolves to an
/// `Unknown` platform. The previous encoding silently substituted the Linux
/// stack, which would stamp a Windows/macOS persona with a Unix TTL (64), the
/// exact G017 transport tell, invisibly. A const panic surfaces the
/// mis-modeled persona instead, and is caught at compile time for any
/// const-evaluated call site.
#[must_use]
#[allow(clippy::panic)]
pub const fn profile_os_network_stack(profile: StealthProfile) -> OsNetworkStack {
    match os_network_stack(profile_platform(profile)) {
        Some(stack) => stack,
        None => panic!(
            "profile_os_network_stack: persona resolves to an Unknown platform with no \
             modeled TCP/IP stack; refusing to silently emit a Linux stack (the G017 tell)"
        ),
    }
}

/// Recover the OS initial TTL from an observed packet TTL by rounding up past
/// the router-hop count.
///
/// Router hops strictly decrease TTL, so the originating OS default is the
/// smallest canonical value (`64`/`128`/`255`) not below the observed TTL.
/// Returns `0` for an unmeasurable TTL of `0`.
#[must_use]
pub const fn infer_initial_ttl(observed_ttl: u8) -> u8 {
    if observed_ttl == 0 {
        0
    } else if observed_ttl <= 64 {
        64
    } else if observed_ttl <= 128 {
        128
    } else {
        255
    }
}

/// Verdict from comparing a persona's claimed OS against an observed wire TTL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkOsCoherence {
    /// The observed initial TTL matches the persona's claimed OS family.
    Coherent {
        /// OS family the persona claims (and the wire agrees with).
        os: UserAgentPlatform,
        /// The matched initial TTL.
        initial_ttl: u8,
    },
    /// The observed initial TTL contradicts the persona's claimed OS family
    /// the classic Windows-persona-on-a-Linux-host transport tell (claimed 128,
    /// wire says 64).
    Mismatch {
        /// OS family the persona claims.
        expected_os: UserAgentPlatform,
        /// Initial TTL that claimed OS would emit.
        expected_ttl: u8,
        /// Initial TTL inferred from the observed packet.
        observed_initial_ttl: u8,
    },
    /// The observed TTL was not measurable (`0`).
    Unknown,
}

impl NetworkOsCoherence {
    /// Whether the persona's network layer agrees with its claimed OS.
    #[must_use]
    pub const fn is_coherent(&self) -> bool {
        matches!(self, Self::Coherent { .. })
    }
}

/// Check whether an observed wire TTL is coherent with a persona's claimed OS.
///
/// The observer supplies the raw TTL seen on a received packet; this de-hops it
/// with [`infer_initial_ttl`] and compares to the persona's expected initial
/// TTL. This is a *reporting* predicate, it states whether the layers agree, it
/// does not assert anything about exploitability.
#[must_use]
pub const fn os_network_coherence(profile: StealthProfile, observed_ttl: u8) -> NetworkOsCoherence {
    let observed_initial = infer_initial_ttl(observed_ttl);
    if observed_initial == 0 {
        return NetworkOsCoherence::Unknown;
    }
    let stack = profile_os_network_stack(profile);
    if stack.initial_ttl == observed_initial {
        NetworkOsCoherence::Coherent {
            os: stack.os,
            initial_ttl: stack.initial_ttl,
        }
    } else {
        NetworkOsCoherence::Mismatch {
            expected_os: stack.os,
            expected_ttl: stack.initial_ttl,
            observed_initial_ttl: observed_initial,
        }
    }
}

/// Whether an observed TCP options layout matches the persona's claimed OS.
///
/// Secondary, corroborating discriminator to the TTL check: the SYN option
/// order (`olayout`) differs by OS family and is harder to normalize than TTL.
#[must_use]
pub fn os_network_options_match(profile: StealthProfile, observed_olayout: &str) -> bool {
    let expected = profile_os_network_stack(profile).tcp_options_layout;
    if expected == observed_olayout {
        return true;
    }
    let exp_tokens = expected.split(',').map(str::trim);
    let obs_tokens = observed_olayout.split(',').map(str::trim);
    exp_tokens.eq(obs_tokens)
}

#[cfg(test)]
#[path = "os_network/tests.rs"]
mod tests;
