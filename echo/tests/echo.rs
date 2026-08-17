use std::net::SocketAddr;
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// A test-only certificate verifier that accepts the self-signed cert.
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // Returning an empty list removes the signature_algorithms extension from
        // the ClientHello and causes the server to abort with NoSignatureSchemes.
        // Accept the common schemes so the handshake can proceed.
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

#[tokio::test]
async fn echo_server_reports_client_hello_ja3_ja4_and_alpn() {
    // Rustls 0.23 requires an explicit crypto provider when both `ring` and
    // `aws-lc-rs` are present in the dependency graph. Install before either the
    // server or the client builds a config.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Bind to an ephemeral port so parallel test runs do not collide.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    // Spawn the server side using the library's accept loop, but fed from our
    // already-bound listener so we control the port.
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await.unwrap()
    });

    // Build a TLS 1.3 client that advertises http/1.1 ALPN and ignores the
    // self-signed certificate.
    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls_stream = connector.connect(server_name, stream).await.unwrap();

    tls_stream
        .write_all(b"GET /echo HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    let mut response = Vec::new();
    tls_stream.read_to_end(&mut response).await.unwrap();

    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response has a body separator");
    let body = &response[body_start + 4..];
    let info: serde_json::Value = serde_json::from_slice(body).expect("response body is JSON");
    let tls = info.get("tls").expect("response has tls field");

    assert!(
        tls.get("ja3")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "ja3 must be a non-empty string"
    );
    assert!(
        tls.get("ja4")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "ja4 must be a non-empty string"
    );

    let alpn = tls.get("alpn").expect("alpn field present");
    assert_eq!(
        alpn,
        &serde_json::json!([[104, 116, 116, 112, 47, 49, 46, 49]]),
        "client advertised http/1.1, server must report it"
    );

    let ja4 = tls.get("ja4").unwrap().as_str().unwrap();
    assert!(
        ja4.starts_with("t13"),
        "TLS 1.3 client JA4 must start with t13: {ja4}"
    );

    // http/1.1 was negotiated, so the H2 field must be null.
    assert!(
        info.get("h2").map(|v| v.is_null()).unwrap_or(true),
        "http/1.1 negotiation must not produce H2 frame info"
    );

    // The TCP layer must report host-level diagnostics, or honestly None.
    let tcp = info.get("tcp").expect("tcp field present");
    if cfg!(target_os = "linux") {
        assert!(
            tcp["host_ttl"].as_u64().is_some(),
            "Linux host TTL must be reported"
        );
    }

    // The server task returned after closing the connection.
    let _info = server.await.unwrap();
}

#[tokio::test]
async fn echo_server_reports_h2_preface_settings_and_window_update() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await.unwrap()
    });

    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls_stream = connector.connect(server_name, stream).await.unwrap();

    // Send the HTTP/2 connection preface.
    tls_stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();

    // Send a SETTINGS frame with three typical browser parameters.
    let settings = [
        (0x01u16, 65_536u32), // HEADER_TABLE_SIZE
        (0x03, 100),          // MAX_CONCURRENT_STREAMS
        (0x04, 6_291_456),    // INITIAL_WINDOW_SIZE
    ];
    tls_stream
        .write_all(&encode_h2_settings(&settings))
        .await
        .unwrap();

    // Send a connection-level WINDOW_UPDATE.
    tls_stream
        .write_all(&encode_h2_window_update(0, 1_000_000))
        .await
        .unwrap();

    // Signal to the server that no more H2 frames are coming so it can echo
    // back the captured fingerprint instead of waiting forever for requests.
    tls_stream.shutdown().await.unwrap();

    // The server is not a full H2 implementation; it reads the preface + frames
    // and then returns its JSON diagnostics over HTTP/1.1 framing.
    let mut response = Vec::new();
    tls_stream.read_to_end(&mut response).await.unwrap();

    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response has a body separator");
    let body = &response[body_start + 4..];
    let info: serde_json::Value = serde_json::from_slice(body).expect("response body is JSON");

    let h2 = info.get("h2").expect("h2 field present").clone();
    assert!(
        h2["preface_seen"].as_bool().unwrap(),
        "server must see H2 preface"
    );

    let tls = info.get("tls").expect("tls field present");
    assert_eq!(
        tls["alpn"],
        serde_json::json!([[104, 50]]),
        "client advertised h2"
    );

    let tcp = info.get("tcp").expect("tcp field present");
    if cfg!(target_os = "linux") {
        assert!(
            tcp["host_ttl"].as_u64().is_some_and(|v| v > 0),
            "Linux host TTL must be a positive byte"
        );
        assert!(
            tcp["timestamps_enabled"].is_boolean()
                && tcp["sack_enabled"].is_boolean()
                && tcp["window_scaling_enabled"].is_boolean(),
            "Linux TCP option knobs must be reported as booleans"
        );
    }

    let settings_json = h2["settings"].as_array().expect("settings array");
    assert_eq!(settings_json.len(), 3, "server must capture all SETTINGS");
    assert_eq!(settings_json[0]["id"], 1);
    assert_eq!(settings_json[0]["value"], 65_536);
    assert_eq!(settings_json[1]["id"], 3);
    assert_eq!(settings_json[1]["value"], 100);
    assert_eq!(settings_json[2]["id"], 4);
    assert_eq!(settings_json[2]["value"], 6_291_456);

    let windows_json = h2["window_updates"]
        .as_array()
        .expect("window_updates array");
    assert_eq!(windows_json.len(), 1);
    assert_eq!(windows_json[0], 1_000_000);

    let _info = server.await.unwrap();
}

fn encode_h2_settings(settings: &[(u16, u32)]) -> Vec<u8> {
    let length = settings.len() * 6;
    let mut frame = Vec::with_capacity(9 + length);
    frame.extend_from_slice(&((length as u32).to_be_bytes()[1..])); // 3-byte length
    frame.push(0x04); // SETTINGS
    frame.push(0x00); // flags
    frame.extend_from_slice(&0u32.to_be_bytes()); // stream id 0
    for (id, value) in settings {
        frame.extend_from_slice(&id.to_be_bytes());
        frame.extend_from_slice(&value.to_be_bytes());
    }
    frame
}

fn encode_h2_window_update(stream_id: u32, increment: u32) -> Vec<u8> {
    let mut frame = Vec::with_capacity(13);
    frame.extend_from_slice(&4u32.to_be_bytes()[1..]); // 3-byte length
    frame.push(0x08); // WINDOW_UPDATE
    frame.push(0x00); // flags
    frame.extend_from_slice(&(stream_id & 0x7fff_ffff).to_be_bytes());
    frame.extend_from_slice(&(increment & 0x7fff_ffff).to_be_bytes());
    frame
}

fn encode_h2_priority(stream_id: u32, dependency: u32, exclusive: bool, weight: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(14);
    frame.extend_from_slice(&5u32.to_be_bytes()[1..]); // 3-byte length
    frame.push(0x02); // PRIORITY
    frame.push(0x00); // flags
    frame.extend_from_slice(&(stream_id & 0x7fff_ffff).to_be_bytes());
    let dep = dependency & 0x7fff_ffff | if exclusive { 0x8000_0000 } else { 0 };
    frame.extend_from_slice(&dep.to_be_bytes());
    frame.push(weight);
    frame
}

#[tokio::test]
async fn echo_server_captures_h2_priority_frame() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await.unwrap()
    });

    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls_stream = connector.connect(server_name, stream).await.unwrap();

    tls_stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();

    let settings = [(0x04u16, 6_291_456u32)];
    tls_stream
        .write_all(&encode_h2_settings(&settings))
        .await
        .unwrap();

    tls_stream
        .write_all(&encode_h2_priority(0x0d, 0x00, true, 110))
        .await
        .unwrap();

    tls_stream.shutdown().await.unwrap();

    let mut response = Vec::new();
    tls_stream.read_to_end(&mut response).await.unwrap();

    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response has a body separator");
    let body = &response[body_start + 4..];
    let info: serde_json::Value = serde_json::from_slice(body).expect("response body is JSON");

    let h2 = info.get("h2").expect("h2 field present");
    assert!(h2["preface_seen"].as_bool().unwrap());

    let priorities = h2["priorities"].as_array().expect("priorities array");
    assert_eq!(priorities.len(), 1);
    assert_eq!(priorities[0]["stream_id"], 0x0d);
    assert_eq!(priorities[0]["exclusive"], true);
    assert_eq!(priorities[0]["dependency"], 0);
    assert_eq!(priorities[0]["weight"], 110);

    let _info = server.await.unwrap();
}

#[tokio::test]
async fn echo_server_reports_missing_h2_preface() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await.unwrap()
    });

    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls_stream = connector.connect(server_name, stream).await.unwrap();

    // Close the write side immediately without sending the H2 preface.
    tls_stream.shutdown().await.unwrap();

    let mut response = Vec::new();
    tls_stream.read_to_end(&mut response).await.unwrap();

    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response has a body separator");
    let body = &response[body_start + 4..];
    let info: serde_json::Value = serde_json::from_slice(body).expect("response body is JSON");

    let h2 = info.get("h2").expect("h2 field present");
    assert!(
        !h2["preface_seen"].as_bool().unwrap(),
        "missing preface must be reported"
    );

    let _info = server.await.unwrap();
}

#[tokio::test]
async fn echo_server_preserves_non_utf8_alpn_and_sni_bytes() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await.unwrap()
    });

    // Non-UTF8 ALPN bytes (0x80 0x81 0x82) must be retained exactly in the
    // advertised list. Include http/1.1 as a fallback so the server (which
    // only advertises h2/http/1.1) can negotiate a compatible protocol.
    let non_utf8_alpn = vec![0x80u8, 0x81, 0x82];
    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![non_utf8_alpn.clone(), b"http/1.1".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls_stream = connector.connect(server_name, stream).await.unwrap();

    tls_stream
        .write_all(b"GET /echo HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    let mut response = Vec::new();
    tls_stream.read_to_end(&mut response).await.unwrap();

    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response has a body separator");
    let body = &response[body_start + 4..];
    let info: serde_json::Value = serde_json::from_slice(body).expect("response body is JSON");
    let tls = info.get("tls").expect("response has tls field");

    assert_eq!(
        tls["alpn"],
        serde_json::json!([[128, 129, 130], [104, 116, 116, 112, 47, 49, 46, 49]]),
        "non-UTF8 ALPN bytes must be retained as raw wire bytes"
    );

    // SNI "localhost" is also captured as raw bytes now.
    assert_eq!(
        tls["sni"],
        serde_json::json!([108, 111, 99, 97, 108, 104, 111, 115, 116]),
        "SNI hostname must be retained as raw wire bytes"
    );

    let _info = server.await.unwrap();
}
