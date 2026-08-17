//! Adversarial tests for guise-echo: hostile bytes on the wire must error
//! cleanly, and a hostile connection must never take down the listener that
//! serves later, honest clients. The echo service exists to measure our own
//! fingerprint, so a crash-or-hang here blinds the whole stealth stack.

use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
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
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// Drive one accepted connection through the library entry point and return
/// the error string (the adversarial cases here must all fail, never panic
/// or hang the caller).
async fn drive_hostile(payload: &[u8], shutdown: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        guise_echo::accept_one_for_test(stream).await
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(payload).await.unwrap();
    if shutdown {
        client.shutdown().await.unwrap();
    }
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("server hung on hostile input")
        .expect("server task panicked");
    result
        .expect_err("hostile input must be rejected")
        .to_string()
}

#[tokio::test]
async fn non_tls_first_byte_is_rejected() {
    let err = drive_hostile(b"GET / HTTP/1.1\r\n\r\n", true).await;
    assert!(
        err.contains("0x16") || err.contains("handshake"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn oversized_record_length_is_rejected() {
    // 0x16 handshake record claiming a 0x4001-byte fragment (limit is 2^14).
    let err = drive_hostile(&[0x16, 0x03, 0x01, 0x40, 0x01], true).await;
    assert!(
        err.contains("exceeds maximum fragment"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn truncated_record_is_rejected_not_hung() {
    // Valid header promising 100 bytes, then the client closes with nothing.
    let err = drive_hostile(&[0x16, 0x03, 0x03, 0x00, 0x64], true).await;
    assert!(
        err.contains("ClientHello") || err.contains("read") || err.contains("parse"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn garbage_record_payload_is_rejected() {
    // Well-formed header, 64 bytes of non-TLS garbage as the fragment.
    let mut payload = vec![0x16, 0x03, 0x03, 0x00, 0x40];
    payload.extend_from_slice(&[0xAA; 64]);
    let err = drive_hostile(&payload, true).await;
    assert!(
        err.contains("ClientHello") || err.contains("tls-parser"),
        "unexpected error: {err}"
    );
}

/// A hostile connection must not kill the accept loop: after sending
/// garbage, a real TLS client still gets a fingerprint answer from the same
/// listener.
#[tokio::test]
async fn listener_survives_hostile_connection() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // serve binds its own listener below

    let server = tokio::spawn(async move {
        let _ = guise_echo::serve(addr).await;
    });
    // `serve` binds inside the task, so the first connect may race the bind.
    // Retry until the listener is up (deadline-bounded); after that the
    // hostile probe synchronizes the rest of the test by behavior.
    let mut hostile = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match TcpStream::connect(addr).await {
                Ok(stream) => break stream,
                Err(err) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "serve never started listening: {err}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        }
    };
    hostile.write_all(b"\x00garbage").await.unwrap();
    hostile.shutdown().await.unwrap();

    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let connector = TlsConnector::from(Arc::new(config));

    let stream = tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .expect("listener died after hostile connection")
        .unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls = connector.connect(server_name, stream).await.unwrap();
    tls.write_all(b"GET /echo HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();
    assert!(
        response.contains(&b'{'),
        "no JSON fingerprint answer after hostile probe: {} bytes",
        response.len()
    );

    server.abort();
}
