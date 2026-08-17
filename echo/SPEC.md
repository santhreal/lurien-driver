# guise-echo Specification

`guise-echo` is a diagnostic server and library in the Santh runtime stack designed to measure, verify, and diff Layer-2/3/4 wire fingerprints emitted by client stealth personas.

---

## 1. System Role & Boundaries

```
[ Stealth Client / scanclient ]
             |
             | TLS 1.3 / HTTP/2
             v
+------------------------------------------+
|               guise-echo                 |
|  1. Read record header (0x16) & payload |
|  2. Parse ClientHello & compute JA3/JA4  |
|  3. Complete TLS Handshake               |
|  4. Capture H2 Preface & Control Frames  |
|  5. Inspect Host TCP /proc knobs         |
|  6. Emit HTTP/1.1 200 JSON Response      |
+------------------------------------------+
```

`guise-echo` operates as a self-contained local reflector. It MUST NOT establish outgoing connections or depend on external services.

---

## 2. Protocol Interception Pipeline

### 2.1 TLS ClientHello Capture

1. **Record Header Read**:
   - Reads exact 5 bytes of initial TLS record header from `TcpStream`.
   - Validates `record[0] == 0x16` (Handshake record type).
   - Extracts 16-bit big-endian payload length $L = \text{be\_u16}(\text{record}[3..5])$.
   - Asserts $L \le 16384$ ($2^{14}$ maximum TLS fragment length). Returns error if exceeded.

2. **Payload Capture & Buffering**:
   - Reads exact $L$ payload bytes into buffer.
   - Wraps `TcpStream` into `BufferedStream<TcpStream>`, prepending the 5 + $L$ read bytes so `tokio-rustls` receives a complete, untouched stream during TLS handshake.

3. **Field Extraction & JA3/JA4 Computation**:
   - Parses TLS record via `tls-parser::parse_tls_plaintext` and `tls-parser::parse_tls_extensions`.
   - Extracts client-ordered vectors: `cipher_suites`, `extensions`, `supported_groups`, `ec_point_formats`, `alpn`, `signature_algorithms`, `supported_versions`, and `sni`.
   - Constructs `guise::fingerprint::ja3::ClientHelloFields` and derives JA3 string and JA4 string.

### 2.2 HTTP/2 Control Frame Capture

Triggered ONLY if ALPN negotiated protocol is `b"h2"`.

1. **Preface Validation**:
   - Reads exact 24 bytes from TLS stream.
   - Validates matching magic bytes `PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`.
   - Sets `preface_seen = true`.

2. **Frame Loop Constraints**:
   - Maximum frame iterations: 32.
   - Maximum single frame payload size: 16,384 bytes.
   - Stop condition: `SETTINGS` frame with `ACK` flag (`flags & 0x01 != 0`), or request stream frames (`DATA` `0x00`, `HEADERS` `0x01`, `CONTINUATION` `0x09`).

3. **Frame Parsing**:
   - `SETTINGS` (`0x04`): Reads 6-byte setting chunks `(u16 id, u32 value)`. Rejects non-multiples of 6.
   - `WINDOW_UPDATE` (`0x08`): Validates 4-byte payload. Extracts 31-bit increment (`payload & 0x7fff_ffff`). Rejects zero increment.
   - `PRIORITY` (`0x02`): Validates 5-byte payload. Extracts dependency, exclusive bit, and weight.

### 2.3 Host TCP Diagnostics

`read_host_tcp_info()` inspects host TCP stack knobs via `/proc/sys/net/ipv4/`:
- `ip_default_ttl` -> `host_ttl: Option<u8>`
- `tcp_timestamps` -> `timestamps_enabled: Option<bool>`
- `tcp_sack` -> `sack_enabled: Option<bool>`
- `tcp_window_scaling` -> `window_scaling_enabled: Option<bool>`

On non-Linux platforms, returns all `None` values (fail-closed, no synthetic defaults).

---

## 3. Data Invariants & Security Boundaries

1. **Unsafe Code Ban**: `#![forbid(unsafe_code)]` enforced at package root.
2. **Panic Safety**: All error paths return `anyhow::Result` or are trapped in per-connection async tokio tasks. Non-fatal H2 frame parse errors log warnings and return captured partial data rather than aborting.
3. **Listener Isolation**: Bad ClientHello records, non-TLS protocol probes, or corrupted frames affect only the local connection instance. The outer `serve` listener loop continues serving subsequent clients.

---

## 4. Response Serialization

Responses are written over the established TLS channel as HTTP/1.1:

```http
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: <length>
Connection: close

<JSON body>
```

JSON payload maps directly to the `ConnectionInfo` Rust structure.
