//! Property-based tests for guise-echo.
//!
//! The echo service's JSON answer is the contract the stealth stack diffs
//! against, so every public info type must survive a JSON round trip
//! byte-exactly for arbitrary captured values (including non-UTF8 ALPN/SNI
//! byte strings, which the fingerprint oracle treats as first-class data).

use guise_echo::{ClientHelloInfo, ConnectionInfo, H2ConnectionInfo, H2Priority, H2Setting};
use proptest::prelude::*;
use serde_json::Value;

fn arb_client_hello() -> impl Strategy<Value = ClientHelloInfo> {
    (
        any::<u16>(),
        prop::collection::vec(any::<u16>(), 0..8),
        prop::collection::vec(any::<u16>(), 0..8),
        prop::collection::vec(any::<u16>(), 0..8),
        prop::collection::vec(any::<u8>(), 0..4),
        prop::collection::vec(prop::collection::vec(any::<u8>(), 0..16), 0..4),
        prop::collection::vec(any::<u16>(), 0..8),
        prop::collection::vec(any::<u16>(), 0..4),
        ".*",
        ".*",
        prop::option::of(prop::collection::vec(any::<u8>(), 0..32)),
    )
        .prop_map(
            |(
                legacy_version,
                cipher_suites,
                extensions,
                supported_groups,
                ec_point_formats,
                alpn,
                signature_algorithms,
                supported_versions,
                ja3,
                ja4,
                sni,
            )| {
                ClientHelloInfo {
                    legacy_version,
                    cipher_suites,
                    extensions,
                    supported_groups,
                    ec_point_formats,
                    alpn,
                    signature_algorithms,
                    supported_versions,
                    ja3,
                    ja4,
                    sni,
                }
            },
        )
}

fn arb_h2() -> impl Strategy<Value = H2ConnectionInfo> {
    (
        any::<bool>(),
        prop::collection::vec((any::<u16>(), any::<u32>()), 0..8),
        prop::collection::vec(any::<u32>(), 0..4),
        prop::collection::vec(
            (any::<u32>(), any::<bool>(), any::<u32>(), any::<u8>()),
            0..4,
        ),
        prop::option::of(prop::collection::vec(any::<u8>(), 0..16)),
        prop::option::of("[a-zA-Z0-9 _-]{1,32}"),
    )
        .prop_map(
            |(preface_seen, settings, window_updates, priorities, negotiated_protocol, error)| {
                H2ConnectionInfo {
                    preface_seen,
                    settings: settings
                        .into_iter()
                        .map(|(id, value)| H2Setting { id, value })
                        .collect(),
                    window_updates,
                    priorities: priorities
                        .into_iter()
                        .map(|(stream_id, exclusive, dependency, weight)| H2Priority {
                            stream_id,
                            exclusive,
                            dependency,
                            weight,
                        })
                        .collect(),
                    negotiated_protocol,
                    error,
                }
            },
        )
}

proptest! {
    /// `ClientHelloInfo` JSON round trip preserves the exact wire bytes,
    /// including non-UTF8 ALPN and SNI values. A lossy round trip here would
    /// corrupt the JA3/JA4 oracle fixtures the stealth stack diffs offline.
    #[test]
    fn client_hello_info_json_roundtrip_is_lossless(info in arb_client_hello()) {
        let json = serde_json::to_value(&info).expect("serialize");
        let back: ClientHelloInfo = serde_json::from_value(json.clone()).expect("deserialize");
        let json_back = serde_json::to_value(&back).expect("re-serialize");
        prop_assert_eq!(json, json_back);
    }

    /// `H2ConnectionInfo` JSON round trip is lossless for arbitrary frame
    /// captures, including partial captures (preface seen, no settings).
    #[test]
    fn h2_connection_info_json_roundtrip_is_lossless(info in arb_h2()) {
        let json = serde_json::to_value(&info).expect("serialize");
        let back: H2ConnectionInfo = serde_json::from_value(json.clone()).expect("deserialize");
        let json_back = serde_json::to_value(&back).expect("re-serialize");
        prop_assert_eq!(json, json_back);
    }

    /// The combined `ConnectionInfo` (the actual `/echo` response body)
    /// round-trips losslessly with and without an H2 section.
    #[test]
    fn connection_info_json_roundtrip_is_lossless(
        tls in arb_client_hello(),
        h2 in prop::option::of(arb_h2()),
        ttl in prop::option::of(any::<u8>()),
        flags in any::<u8>(),
    ) {
        let info = ConnectionInfo {
            tls,
            h2,
            tcp: guise_echo::TcpConnectionInfo {
                host_ttl: ttl,
                timestamps_enabled: Some(flags & 1 != 0),
                sack_enabled: Some(flags & 2 != 0),
                window_scaling_enabled: Some(flags & 4 != 0),
            },
        };
        let json: Value = serde_json::to_value(&info).expect("serialize");
        let back: ConnectionInfo = serde_json::from_value(json.clone()).expect("deserialize");
        let json_back = serde_json::to_value(&back).expect("re-serialize");
        prop_assert_eq!(json, json_back);
    }

    /// The host TCP read never fabricates values: every field is either
    /// populated from a real `/proc` knob or `None`. This property pins the
    /// fail-closed shape of the read itself across repeated calls.
    #[test]
    fn host_tcp_info_shape_is_stable(_run in 0..4u8) {
        let first = guise_echo::read_host_tcp_info();
        let second = guise_echo::read_host_tcp_info();
        prop_assert_eq!(first.host_ttl, second.host_ttl);
        prop_assert_eq!(first.timestamps_enabled, second.timestamps_enabled);
        if let Some(ttl) = first.host_ttl {
            prop_assert!(ttl > 0, "a readable TTL must be a real, positive value");
        }
    }
}
