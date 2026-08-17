//! Local TLS echo service for inspecting our own wire fingerprints.
//!
//! `guise-echo` terminates TLS 1.3 locally, parses the client's raw ClientHello
//! with `tls-parser`, computes JA3/JA4 via `guise::fingerprint::ja3`, and
//! returns the result as JSON over HTTP/1.1. It exists so the stealth stack can
//! inspect its own Layer-2 bytes without relying on third-party reflectors.
//!
//! Layers:
//!   * TLS ClientHello capture (JA3/JA4, ALPN, SNI, extensions)
//!   * HTTP/2 preface + SETTINGS/WINDOW_UPDATE/PRIORITY frame capture
//!   * TCP TTL/options read (`tcp::read_host_tcp_info`)
//!
//! The service intentionally does not implement a full HTTP/2 server; it only
//! reads the connection preface and the first control frames so the stealth
//! stack can verify that its emitted H2 fingerprint matches a real browser.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{bail, Context as _};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

pub mod tcp;
pub use tcp::{read_host_tcp_info, TcpConnectionInfo};

pub use serde_json::Value as JsonValue;

/// Parsed TLS ClientHello metadata + computed fingerprints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHelloInfo {
    /// TLS legacy version field.
    pub legacy_version: u16,
    /// Cipher suites, in client order.
    pub cipher_suites: Vec<u16>,
    /// TLS extension type ids, in client order.
    pub extensions: Vec<u16>,
    /// Supported elliptic-curve / named groups.
    pub supported_groups: Vec<u16>,
    /// EC point formats.
    pub ec_point_formats: Vec<u8>,
    /// ALPN protocols, in client order (raw wire bytes).
    pub alpn: Vec<Vec<u8>>,
    /// Signature algorithms, in client order.
    pub signature_algorithms: Vec<u16>,
    /// TLS versions from the `supported_versions` extension (0x002b).
    pub supported_versions: Vec<u16>,
    /// Computed JA3 string.
    pub ja3: String,
    /// Computed JA4 string.
    pub ja4: String,
    /// SNI hostname, if present (raw wire bytes).
    pub sni: Option<Vec<u8>>,
}

/// Captured HTTP/2 connection-level information from the client preface and
/// first control frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct H2ConnectionInfo {
    /// Whether the client sent the magic `PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n` preface.
    pub preface_seen: bool,
    /// SETTINGS id/value pairs, in arrival order.
    pub settings: Vec<H2Setting>,
    /// WINDOW_INCREMENT values from WINDOW_UPDATE frames on the connection
    /// stream (stream id 0).
    pub window_updates: Vec<u32>,
    /// PRIORITY frames captured before the first request.
    pub priorities: Vec<H2Priority>,
    /// The negotiated ALPN protocol that triggered H2 frame reading (raw bytes).
    pub negotiated_protocol: Option<Vec<u8>>,
    /// Capture error string if reading preface/frames stopped due to a protocol error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A single HTTP/2 SETTINGS parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct H2Setting {
    pub id: u16,
    pub value: u32,
}

/// A single HTTP/2 PRIORITY frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct H2Priority {
    pub stream_id: u32,
    pub exclusive: bool,
    pub dependency: u32,
    pub weight: u8,
}

/// Combined fingerprint info returned by the echo endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub tls: ClientHelloInfo,
    pub h2: Option<H2ConnectionInfo>,
    pub tcp: TcpConnectionInfo,
}

/// Run the echo server on `bind` until the process is killed.
pub async fn serve(bind: SocketAddr) -> anyhow::Result<()> {
    let tls_config = Arc::new(server_config()?);
    let listener = TcpListener::bind(bind).await?;
    tracing::info!("guise-echo listening on {bind}");

    loop {
        let (stream, peer) = listener.accept().await?;
        let cfg = tls_config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, cfg).await {
                tracing::debug!(%peer, "connection closed with error: {e:#}");
            }
        });
    }
}

/// Accept a single TCP connection, parse its TLS ClientHello, complete the
/// handshake, and echo the parsed fingerprint as JSON. Exposed for tests that
/// want to drive the server side without binding a listening socket.
pub async fn accept_one_for_test(stream: TcpStream) -> anyhow::Result<ConnectionInfo> {
    let tls_config = Arc::new(server_config()?);
    handle_connection(stream, tls_config).await
}

async fn handle_connection(
    stream: TcpStream,
    tls_config: Arc<ServerConfig>,
) -> anyhow::Result<ConnectionInfo> {
    let (tls_info, buffered) = read_and_parse_client_hello(stream).await?;

    let acceptor = TlsAcceptor::from(tls_config);
    let mut tls_stream = acceptor.accept(buffered).await?;

    let h2_info = read_h2_info_if_negotiated(&mut tls_stream).await?;
    let tcp_info = read_host_tcp_info();

    let info = ConnectionInfo {
        tls: tls_info,
        h2: h2_info,
        tcp: tcp_info,
    };

    let body = serde_json::to_vec(&info)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    tls_stream.write_all(response.as_bytes()).await?;
    tls_stream.write_all(&body).await?;
    tls_stream.shutdown().await?;
    Ok(info)
}

/// If the TLS handshake negotiated `h2`, read and parse the HTTP/2 connection
/// preface and the first control frames. Non-fatal parse errors are logged and
/// surfaced in the returned struct so tests can see partial data.
async fn read_h2_info_if_negotiated<S>(
    tls_stream: &mut tokio_rustls::server::TlsStream<S>,
) -> anyhow::Result<Option<H2ConnectionInfo>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let negotiated = tls_stream.get_ref().1.alpn_protocol().map(|p| p.to_vec());

    if negotiated.as_deref() != Some(b"h2".as_slice()) {
        return Ok(None);
    }

    let mut h2 = H2ConnectionInfo {
        preface_seen: false,
        settings: Vec::new(),
        window_updates: Vec::new(),
        priorities: Vec::new(),
        negotiated_protocol: negotiated,
        error: None,
    };

    // Read the fixed 24-byte preface, then the first few control frames.
    match read_h2_preface_and_frames(tls_stream, &mut h2).await {
        Ok(()) => Ok(Some(h2)),
        Err(e) => {
            tracing::debug!("H2 frame capture stopped: {e:#}");
            h2.error = Some(e.to_string());
            Ok(Some(h2))
        }
    }
}

/// Read the first TLS record from `stream`, parse the ClientHello, and return
/// both the parsed info and a stream that has the consumed bytes prepended so
/// `tokio-rustls` can complete the handshake.
async fn read_and_parse_client_hello(
    mut stream: TcpStream,
) -> anyhow::Result<(ClientHelloInfo, BufferedStream<TcpStream>)> {
    // Read the 5-byte record header directly. `peek` was avoided on purpose:
    // a single peek guarantees only one byte, so a client (or middlebox) that
    // fragments the header across TCP segments could leave the length bytes
    // zeroed and the record length below would be garbage. `read_exact` waits
    // for the full header without busy-polling.
    let mut record = vec![0u8; 5];
    stream
        .read_exact(&mut record)
        .await
        .context("read TLS record header")?;
    if record[0] != 0x16 {
        bail!(
            "first byte is 0x{:02x}, expected 0x16 (handshake record)",
            record[0]
        );
    }
    let record_len = u16::from_be_bytes([record[3], record[4]]) as usize;
    if record_len > 1 << 14 {
        bail!("TLS record length {record_len} exceeds maximum fragment size");
    }

    record.resize(5 + record_len, 0);
    stream
        .read_exact(&mut record[5..])
        .await
        .context("read TLS ClientHello record")?;

    let info = parse_client_hello(&record).context("parse ClientHello")?;
    let buffered = BufferedStream::new(record, stream);
    Ok((info, buffered))
}

fn parse_client_hello(record: &[u8]) -> anyhow::Result<ClientHelloInfo> {
    use tls_parser::{TlsExtension, TlsMessage, TlsMessageHandshake};

    let (rem, parsed) = tls_parser::parse_tls_plaintext(record)
        .map_err(|e| anyhow::anyhow!("tls-parser failed: {e:?}"))?;
    if !rem.is_empty() {
        bail!(
            "trailing unparsed bytes ({}) in TLS record payload",
            rem.len()
        );
    }
    if parsed.msg.len() != 1 {
        bail!(
            "TLS record payload contains {} messages, expected exactly 1",
            parsed.msg.len()
        );
    }

    let ch = match &parsed.msg[0] {
        TlsMessage::Handshake(TlsMessageHandshake::ClientHello(ch)) => ch,
        other => bail!("expected ClientHello, got {other:?}"),
    };
    let cipher_suites: Vec<u16> = ch.ciphers.iter().map(|c| c.0).collect();

    let ext_bytes = ch.ext.unwrap_or(&[]);
    let (rem_ext, parsed_exts) = tls_parser::parse_tls_extensions(ext_bytes)
        .map_err(|e| anyhow::anyhow!("tls-parser extensions failed: {e:?}"))?;
    if !rem_ext.is_empty() {
        bail!(
            "trailing unparsed bytes ({}) in TLS extensions",
            rem_ext.len()
        );
    }

    let mut seen_exts = std::collections::HashSet::new();
    for ext in &parsed_exts {
        let ext_type = u16::from(tls_parser::TlsExtensionType::from(ext));
        if !seen_exts.insert(ext_type) {
            bail!("duplicate TLS extension type 0x{ext_type:04x} in ClientHello");
        }
    }

    let mut alpn = Vec::new();
    let mut supported_versions = Vec::new();
    let mut signature_algorithms = Vec::new();
    let mut supported_groups = Vec::new();
    let mut ec_point_formats = Vec::new();
    let mut sni = None;

    let extensions: Vec<u16> = parsed_exts
        .iter()
        .map(|ext| u16::from(tls_parser::TlsExtensionType::from(ext)))
        .collect();

    for ext in &parsed_exts {
        match ext {
            TlsExtension::ALPN(protocols) => {
                alpn.extend(protocols.iter().map(|p| p.to_vec()));
            }
            TlsExtension::SupportedVersions(versions) => {
                supported_versions.extend(versions.iter().map(|v| v.0));
            }
            TlsExtension::SignatureAlgorithms(algorithms) => {
                signature_algorithms.extend(algorithms.iter().copied());
            }
            TlsExtension::EllipticCurves(groups) => {
                supported_groups.extend(groups.iter().map(|g| g.0));
            }
            TlsExtension::EcPointFormats(formats) => {
                ec_point_formats.extend(formats.iter().copied());
            }
            TlsExtension::SNI(entries) => {
                if let Some((_, hostname)) = entries.first() {
                    sni = Some(hostname.to_vec());
                }
            }
            _ => {}
        }
    }

    let fields = guise::fingerprint::ja3::ClientHelloFields {
        version: ch.version.0,
        cipher_suites: cipher_suites.clone(),
        extensions: extensions.clone(),
        supported_groups: supported_groups.clone(),
        ec_point_formats: ec_point_formats.clone(),
        alpn: alpn.clone(),
        signature_algorithms: signature_algorithms.clone(),
        supported_versions: supported_versions.clone(),
    };

    let ja3 = guise::fingerprint::ja3::compute_ja3(&fields);
    let ja4 = guise::fingerprint::ja3::compute_ja4(&fields);

    Ok(ClientHelloInfo {
        legacy_version: ch.version.0,
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
    })
}

/// Read the HTTP/2 connection preface and parse the first SETTINGS,
/// WINDOW_UPDATE, and PRIORITY frames sent by the client.
///
/// We stop after a small byte/frame budget so a misbehaving client cannot keep
/// this diagnostic service open indefinitely.
async fn read_h2_preface_and_frames<S>(
    stream: &mut S,
    info: &mut H2ConnectionInfo,
) -> anyhow::Result<()>
where
    S: AsyncRead + Unpin,
{
    const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    const MAX_FRAME_BYTES: usize = 16_384;
    const MAX_FRAMES: usize = 32;

    let mut preface_buf = vec![0u8; PREFACE.len()];
    let _ = stream
        .read_exact(&mut preface_buf)
        .await
        .context("read H2 preface")?;
    if preface_buf != PREFACE {
        bail!("invalid H2 preface");
    }
    info.preface_seen = true;

    for _ in 0..MAX_FRAMES {
        let mut header = [0u8; 9];
        match AsyncReadExt::read_exact(&mut *stream, &mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }

        let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let frame_type = header[3];
        let flags = header[4];
        let stream_id =
            u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff;

        if length > MAX_FRAME_BYTES {
            bail!("H2 frame length {length} exceeds limit");
        }

        // A SETTINGS frame with the ACK flag ends the mandatory exchange.
        if frame_type == 0x04 && (flags & 0x01) != 0 {
            if length != 0 {
                bail!("SETTINGS ACK frame length must be 0, got {length}");
            }
            break;
        }

        if frame_type == 0x04 && stream_id != 0 {
            bail!("SETTINGS frame stream ID must be 0, got {stream_id}");
        }

        if frame_type == 0x02 && stream_id == 0 {
            bail!("PRIORITY frame stream ID must not be 0");
        }

        if matches!(frame_type, 0x00 | 0x01 | 0x09) {
            // Stop reading once we see a request-bearing frame (DATA, HEADERS, CONTINUATION).
            break;
        }

        let payload = if length > 0 {
            let mut buf = vec![0u8; length];
            let _ = stream
                .read_exact(&mut buf)
                .await
                .context("read H2 frame payload")?;
            buf
        } else {
            Vec::new()
        };

        match frame_type {
            0x04 => parse_h2_settings(&payload, info)?,
            0x08 => parse_h2_window_update(stream_id, &payload, info)?,
            0x02 => parse_h2_priority(stream_id, &payload, info)?,
            _ => {}
        }
    }

    Ok(())
}

fn parse_h2_settings(payload: &[u8], info: &mut H2ConnectionInfo) -> anyhow::Result<()> {
    if !payload.len().is_multiple_of(6) {
        bail!(
            "SETTINGS payload length {} is not a multiple of 6",
            payload.len()
        );
    }
    let mut seen_ids = std::collections::HashSet::new();
    for chunk in payload.chunks_exact(6) {
        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
        let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
        if !seen_ids.insert(id) {
            bail!("duplicate SETTINGS parameter ID {id} in SETTINGS frame");
        }
        info.settings.push(H2Setting { id, value });
    }
    Ok(())
}

fn parse_h2_window_update(
    stream_id: u32,
    payload: &[u8],
    info: &mut H2ConnectionInfo,
) -> anyhow::Result<()> {
    if payload.len() != 4 {
        bail!(
            "WINDOW_UPDATE payload must be 4 bytes, got {}",
            payload.len()
        );
    }
    let increment =
        u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7fff_ffff;
    if increment == 0 {
        bail!("WINDOW_UPDATE increment is zero");
    }
    // Capture connection-level window updates (stream id 0) and request-stream
    // updates so the fingerprint is complete.
    info.window_updates.push(increment);
    let _ = stream_id; // kept for future per-stream accounting
    Ok(())
}

fn parse_h2_priority(
    stream_id: u32,
    payload: &[u8],
    info: &mut H2ConnectionInfo,
) -> anyhow::Result<()> {
    if payload.len() != 5 {
        bail!("PRIORITY payload must be 5 bytes, got {}", payload.len());
    }
    let dependency = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let exclusive = (dependency & 0x8000_0000) != 0;
    let dependency = dependency & 0x7fff_ffff;
    if dependency == stream_id {
        bail!("PRIORITY frame stream {stream_id} cannot depend on itself");
    }
    let weight = payload[4];
    info.priorities.push(H2Priority {
        stream_id,
        exclusive,
        dependency,
        weight,
    });
    Ok(())
}

fn ensure_crypto_provider() {
    // Rustls 0.23 cannot auto-select a crypto provider when both `ring` and
    // `aws-lc-rs` are present in the dependency graph. We pin the ring provider
    // explicitly and ignore the already-installed error on repeated calls.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn server_config() -> anyhow::Result<ServerConfig> {
    ensure_crypto_provider();
    let cert = include_bytes!("cert.pem").to_vec();
    let key = include_bytes!("key.pem").to_vec();

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert)
        .collect::<Result<Vec<_>, _>>()
        .context("parse cert.pem")?;
    let key = PrivateKeyDer::pem_slice_iter(&key)
        .next()
        .transpose()
        .context("parse key.pem")?
        .context("key.pem contains no PKCS#8 private key")?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build rustls server config")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

/// A stream that first yields a buffered prefix, then delegates to an inner
/// stream. Used to let `tokio-rustls` complete a TLS handshake after we have
/// already consumed and parsed the ClientHello bytes.
struct BufferedStream<S> {
    prefix: Vec<u8>,
    position: usize,
    inner: S,
}

impl<S> BufferedStream<S> {
    fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self {
            prefix,
            position: 0,
            inner,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for BufferedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.position < self.prefix.len() {
            let remaining = self.prefix.len() - self.position;
            let to_copy = remaining.min(buf.remaining());
            buf.put_slice(&self.prefix[self.position..self.position + to_copy]);
            self.position += to_copy;
            Poll::Ready(Ok(()))
        } else {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for BufferedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_stream_yields_prefix_then_inner() {
        // A simple synchronous smoke test for the AsyncRead behaviour via a
        // tokio runtime: the prefix is returned before the inner stream.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let prefix = b"hello ";
            let inner: &[u8] = b"world";
            let mut stream = BufferedStream::new(prefix.to_vec(), inner);
            let mut out = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut out)
                .await
                .unwrap();
            assert_eq!(out, b"hello world");
        });
    }

    /// A minimal but well-formed TLS 1.2-era ClientHello record offering one
    /// cipher suite (0x1301) and no extensions.
    fn minimal_client_hello_record() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]); // legacy version
        body.extend_from_slice(&[0x42; 32]); // random
        body.push(0); // session id length
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // cipher suites
        body.extend_from_slice(&[0x01, 0x00]); // compression methods
        body.extend_from_slice(&[0x00, 0x00]); // extensions length
        let mut handshake = vec![0x01];
        handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend_from_slice(&body);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);
        record
    }

    /// Regression: the ClientHello reader peeked the 5-byte record header
    /// with a single `TcpStream::peek`, which guarantees only ONE byte. A
    /// peer that fragments the header across TCP segments left the length
    /// bytes zeroed, so the reader computed a garbage record length and the
    /// capture failed (or misaligned the stream for the TLS handshake that
    /// reuses those bytes). The reader must wait for the complete header.
    #[tokio::test]
    async fn client_hello_reader_tolerates_fragmented_record_header() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let record = minimal_client_hello_record();

        let client = tokio::spawn(async move {
            let mut conn = TcpStream::connect(addr).await.unwrap();
            // Dribble the header: two bytes, a pause, then the rest.
            conn.write_all(&record[..2]).await.unwrap();
            conn.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            conn.write_all(&record[2..]).await.unwrap();
            conn.flush().await.unwrap();
            // Keep the connection open until the server is done reading.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        let (stream, _) = listener.accept().await.unwrap();
        let (info, _buffered) = read_and_parse_client_hello(stream)
            .await
            .expect("fragmented record header must still parse");
        assert_eq!(info.cipher_suites, vec![0x1301]);
        client.await.unwrap();
    }

    /// Regression: the H2 capture loop checked the empty-payload fast path
    /// BEFORE the SETTINGS-ACK terminator. A SETTINGS ACK is required to be
    /// empty (RFC 9113 §6.5), so it always took the `continue` path and the
    /// ACK never ended the capture: frames after the ACK were wrongly
    /// attributed to the connection preface fingerprint.
    #[tokio::test]
    async fn h2_capture_stops_at_settings_ack() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // SETTINGS, one entry (id=1, value=4096).
        bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x10, 0x00]);
        // SETTINGS ACK (empty, flags=0x1).
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x04, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        // A WINDOW_UPDATE after the ACK must NOT be captured.
        bytes.extend_from_slice(&[0x00, 0x00, 0x04, 0x08, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .expect("preface and frames parse");
        assert!(info.preface_seen);
        assert_eq!(info.settings.len(), 1);
        assert_eq!(info.settings[0].id, 1);
        assert_eq!(info.settings[0].value, 4096);
        assert!(
            info.window_updates.is_empty(),
            "frames after the SETTINGS ACK must not be captured"
        );
    }
    #[tokio::test]
    async fn h2_capture_stops_at_empty_data_frame() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // Empty DATA frame (len=0, type=0, flags=1 END_STREAM, stream_id=1).
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        // Following frame after empty DATA frame must be ignored.
        bytes.extend_from_slice(&[0x00, 0x00, 0x04, 0x08, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .expect("preface and empty DATA frame parse");
        assert!(info.preface_seen);
        assert!(info.window_updates.is_empty());
    }

    #[tokio::test]
    async fn h2_rejects_invalid_settings_ack_length() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // SETTINGS ACK with length=1 (invalid per RFC 9113 §6.5).
        bytes.extend_from_slice(&[0x00, 0x00, 0x01, 0x04, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.push(0x00);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("SETTINGS ACK frame length must be 0"));
    }

    #[tokio::test]
    async fn h2_rejects_settings_frame_with_nonzero_stream_id() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // SETTINGS with stream_id=1 (invalid per RFC 9113 §6.5).
        bytes.extend_from_slice(&[0x00, 0x00, 0x06, 0x04, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x10, 0x00]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("SETTINGS frame stream ID must be 0"));
    }

    #[tokio::test]
    async fn h2_rejects_priority_frame_with_zero_stream_id() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // PRIORITY with stream_id=0 (invalid per RFC 9113 §6.2).
        bytes.extend_from_slice(&[0x00, 0x00, 0x05, 0x02, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x10]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("PRIORITY frame stream ID must not be 0"));
    }

    #[test]
    fn client_hello_rejects_trailing_bytes_in_extensions() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]); // legacy version
        body.extend_from_slice(&[0x42; 32]); // random
        body.push(0); // session id length
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // cipher suites
        body.extend_from_slice(&[0x01, 0x00]); // compression methods
                                               // Extension block length 5: SNI extension header (4 bytes) + 1 trailing byte 0xff
        body.extend_from_slice(&[0x00, 0x05]);
        body.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0xff]);

        let mut handshake = vec![0x01];
        handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend_from_slice(&body);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);

        let err = parse_client_hello(&record).unwrap_err();
        assert!(err.to_string().contains("trailing unparsed bytes"));
    }
    #[tokio::test]
    async fn h2_rejects_zero_length_window_update() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // WINDOW_UPDATE with length=0 (invalid per RFC 9113 §6.9).
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x08, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("WINDOW_UPDATE payload must be 4 bytes, got 0"));
    }

    #[tokio::test]
    async fn h2_rejects_zero_length_priority() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // PRIORITY with length=0 (invalid per RFC 9113 §6.3).
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x02, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("PRIORITY payload must be 5 bytes, got 0"));
    }

    #[tokio::test]
    async fn h2_rejects_duplicate_settings_parameter() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // SETTINGS frame with duplicate ID 1.
        bytes.extend_from_slice(&[0x00, 0x00, 0x0c, 0x04, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x10, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x20, 0x00]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("duplicate SETTINGS parameter ID 1"));
    }

    #[tokio::test]
    async fn h2_rejects_self_dependent_priority() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        // PRIORITY frame on stream 1 depending on stream 1.
        bytes.extend_from_slice(&[0x00, 0x00, 0x05, 0x02, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x10]);

        let mut cursor: &[u8] = &bytes;
        let mut info = H2ConnectionInfo {
            preface_seen: false,
            settings: Vec::new(),
            window_updates: Vec::new(),
            priorities: Vec::new(),
            negotiated_protocol: None,
            error: None,
        };
        let err = read_h2_preface_and_frames(&mut cursor, &mut info)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("PRIORITY frame stream 1 cannot depend on itself"));
    }

    #[test]
    fn client_hello_rejects_duplicate_extension_types() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]); // legacy version
        body.extend_from_slice(&[0x42; 32]); // random
        body.push(0); // session id length
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // cipher suites
        body.extend_from_slice(&[0x01, 0x00]); // compression methods
                                               // Extension block: two supported_versions extensions (0x002b)
        body.extend_from_slice(&[0x00, 0x0e]);
        body.extend_from_slice(&[0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04]);
        body.extend_from_slice(&[0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04]);

        let mut handshake = vec![0x01];
        handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend_from_slice(&body);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);

        let err = parse_client_hello(&record).unwrap_err();
        assert!(err
            .to_string()
            .contains("duplicate TLS extension type 0x002b"));
    }
}
