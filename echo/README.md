# guise-echo: local TLS+H2+TCP echo service for wire fingerprinting

![status: beta](https://img.shields.io/badge/status-beta-yellow)

`guise-echo` terminates local TLS 1.3 connections, parses raw TLS `ClientHello` messages, captures HTTP/2 control frames (preface, `SETTINGS`, `WINDOW_UPDATE`, `PRIORITY`), and reads host TCP/IP stack configuration. It echoes the captured fingerprint metadata as JSON over HTTP/1.1.

`guise-echo` allows the Santh stealth stack (`guise`, `scanclient`, `stealth-core`) to inspect and diff its emitted Layer-2/3/4 bytes locally without external network dependencies or third-party reflection endpoints.

## What it does

1. **TLS ClientHello Capture**
   - Intercepts raw TLS record headers before handshake completion.
   - Parses cipher suites, extension IDs, supported elliptic curves/groups, EC point formats, ALPN values, signature algorithms, and `supported_versions`.
   - Computes canonical JA3 and JA4 fingerprint strings via `guise::fingerprint::ja3`.

2. **HTTP/2 Frame Capture**
   - Evaluates negotiated ALPN (`h2`).
   - Validates client magic connection preface (`PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`).
   - Captures client `SETTINGS` parameter ID/value pairs, `WINDOW_UPDATE` increments, and `PRIORITY` frame parameters within a strict frame budget before tearing down the diagnostic loop.

3. **Host TCP Stack Inspection**
   - Reads Linux TCP stack configuration (`/proc/sys/net/ipv4/*`) for initial SYN TTL, TCP timestamps, SACK permission, and window scaling options to verify operating system signature coherence.

4. **Adversarial Resilience**
   - Fails closed against malformed, oversized, or truncated TLS records.
   - Ensures hostile or malformed probes never panic the server or crash the background listener loop.

## Quick start

### As a Library

Add `guise-echo` to your `Cargo.toml`:

```toml
[dependencies]
guise-echo = { path = "software/browser/echo" }
```

Spin up the echo server:

```rust,no_run
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "127.0.0.1:8443".parse()?;
    guise_echo::serve(addr).await
}
```

### As a Standalone Binary

Run the echo service daemon:

```bash
cargo run -p guise-echo -- 127.0.0.1:8443
```

Query with a TLS client:

```bash
curl -k --http1.1 https://127.0.0.1:8443/
```

## Response Schema

The endpoint responds with HTTP `200 OK` containing a JSON serialized `ConnectionInfo` object:

```json
{
  "tls": {
    "legacy_version": 771,
    "cipher_suites": [4865, 4866, 4867],
    "extensions": [0, 23, 65281, 10, 11, 35, 16, 5, 13, 18, 51, 45, 43, 27],
    "supported_groups": [29, 23, 24],
    "ec_point_formats": [0],
    "alpn": [[104, 50], [104, 116, 116, 112, 47, 49, 46, 49]],
    "signature_algorithms": [1027, 1283, 1539, 2052, 2053, 2054],
    "supported_versions": [772, 771],
    "ja3": "771,4865-4866-4867,0-23-65281-10-11-35-16-5-13-18-51-45-43-27,29-23-24,0",
    "ja4": "t13d151600_8a2a4922e028_9e88b64e03d3",
    "sni": [108, 111, 99, 97, 108, 104, 111, 115, 116]
  },
  "h2": {
    "preface_seen": true,
    "settings": [
      { "id": 1, "value": 65536 },
      { "id": 2, "value": 0 }
    ],
    "window_updates": [15728640],
    "priorities": [],
    "negotiated_protocol": [104, 50]
  },
  "tcp": {
    "host_ttl": 64,
    "timestamps_enabled": true,
    "sack_enabled": true,
    "window_scaling_enabled": true
  }
}
```

## Testing

Run unit, adversarial, and property test suites:

```bash
cargo test -p guise-echo
```

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
