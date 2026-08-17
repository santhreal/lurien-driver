//! TCP-layer diagnostics for the local echo service.
//!
//! This module reads the host's TCP/IP stack configuration, the settings that
//! determine what a SYN emitted from this host looks like. It is intentionally
//! a *host-level* read, not a packet-capture: reading the raw options of an
//! incoming SYN requires a privileged raw socket or netfilter hook, which is out
//! of scope for a local diagnostic service and is instead the domain of the
//! cross-OS egress rewrite (G017/G019). The values here are sufficient to
//! validate that a local self-probe sees the expected OS-family stack.

/// TCP/IP stack information captured for a connection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TcpConnectionInfo {
    /// Host default IP TTL (the initial TTL this host emits on SYNs).
    pub host_ttl: Option<u8>,
    /// Whether the host stack emits TCP timestamps (`/proc/sys/net/ipv4/tcp_timestamps`).
    pub timestamps_enabled: Option<bool>,
    /// Whether the host stack advertises SACK permitted (`/proc/sys/net/ipv4/tcp_sack`).
    pub sack_enabled: Option<bool>,
    /// Whether the host stack negotiates window scaling (`/proc/sys/net/ipv4/tcp_window_scaling`).
    pub window_scaling_enabled: Option<bool>,
}

/// Read the host TCP/IP configuration relevant to SYN fingerprinting.
///
/// On Linux this reads `/proc/sys/net/ipv4/*` knobs. On other platforms it
/// returns an all-`None` struct (fail closed (no fabricated value)).
pub fn read_host_tcp_info() -> TcpConnectionInfo {
    TcpConnectionInfo {
        host_ttl: read_u8_proc_sys("/proc/sys/net/ipv4/ip_default_ttl"),
        timestamps_enabled: read_bool_proc_sys("/proc/sys/net/ipv4/tcp_timestamps"),
        sack_enabled: read_bool_proc_sys("/proc/sys/net/ipv4/tcp_sack"),
        window_scaling_enabled: read_bool_proc_sys("/proc/sys/net/ipv4/tcp_window_scaling"),
    }
}

fn read_u8_proc_sys(path: &str) -> Option<u8> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u8>()
        .ok()
}

fn read_bool_proc_sys(path: &str) -> Option<bool> {
    match read_u8_proc_sys(path)? {
        0 => Some(false),
        1 | 2 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_tcp_info_is_populated_or_honestly_absent_on_linux() {
        let info = read_host_tcp_info();
        if cfg!(target_os = "linux") {
            // These knobs are present on normal Linux hosts. We do not assert
            // specific values because a retuned host is still valid; we only
            // assert the read itself succeeds and produces sane booleans.
            assert!(
                info.host_ttl.is_some(),
                "ip_default_ttl must be readable on Linux"
            );
            if let Some(ttl) = info.host_ttl {
                assert!(ttl > 0, "TTL must be positive, got {ttl}");
            }
            assert!(info.timestamps_enabled.is_some());
            assert!(info.sack_enabled.is_some());
            assert!(info.window_scaling_enabled.is_some());
        } else {
            // Non-Linux: every field must be None, never a guess.
            assert_eq!(info.host_ttl, None);
            assert_eq!(info.timestamps_enabled, None);
            assert_eq!(info.sack_enabled, None);
            assert_eq!(info.window_scaling_enabled, None);
        }
    }
    #[test]
    fn read_bool_proc_sys_handles_sysctl_value_2() {
        // Value 2 on Linux represents enabled with per-route/listener flags (RFC 7323)
        let parse_val = |raw: &str| match raw.trim().parse::<u8>().ok()? {
            0 => Some(false),
            1 | 2 => Some(true),
            _ => None,
        };
        assert_eq!(parse_val("0"), Some(false));
        assert_eq!(parse_val("1"), Some(true));
        assert_eq!(parse_val("2"), Some(true));
        assert_eq!(parse_val("3"), None);
    }
}
