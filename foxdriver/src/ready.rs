/* Readiness of a live WebDriver BiDi endpoint. */

//! Is the remote agent answering commands yet?
//!
//! An open TCP port is not a browser ready to be driven. Gecko binds the remote
//! agent's socket early and answers commands only once the agent's own event loop
//! is running, and on a loaded machine those two moments are seconds apart.
//! rustenium's `session.new` waits a hardcoded five seconds and panics if the
//! answer is late, so a busy host turns a healthy browser into a launch failure.
//!
//! `session.status` is the one command the specification answers without a
//! session, which makes it the readiness question: it costs one WebSocket
//! connection, it creates nothing, and an answer proves the agent's loop is
//! turning. Only then is the endpoint handed to rustenium, whose five seconds now
//! start against a warm agent.
//!
//! The client here is deliberately small: one text frame out, one frame in, no
//! continuation, no compression, no ping. That is the whole conversation, and it
//! keeps this crate free of a WebSocket dependency for a single request.

use anyhow::{anyhow, Result};
use base64::Engine as _;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long one status question may take before it is abandoned and retried.
const ASK_TIMEOUT: Duration = Duration::from_secs(3);

/// Gap between questions. Short enough that a browser that becomes ready is used
/// immediately, long enough that a slow start is not a busy loop.
const ASK_GAP: Duration = Duration::from_millis(250);

/// A frame larger than this from the remote agent is not a status reply.
const MAX_FRAME: usize = 64 * 1024;

/// Wait until the BiDi endpoint answers `session.status`, or `deadline` elapses.
///
/// The error names what was tried, because the caller's next move differs: an
/// endpoint that never answers is a browser that failed to finish starting, not a
/// port that was never bound.
pub async fn wait_until_ready(host: &str, port: u16, deadline: Duration) -> Result<()> {
    let start = Instant::now();
    let mut last = String::from("no answer yet");
    loop {
        match tokio::time::timeout(ASK_TIMEOUT, ask_status(host, port)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => last = e.to_string(),
            Err(_) => last = format!("no reply within {}s", ASK_TIMEOUT.as_secs()),
        }
        if start.elapsed() >= deadline {
            return Err(anyhow!(
                "the browser bound {host}:{port} but never answered session.status within {}s: {last}",
                deadline.as_secs()
            ));
        }
        tokio::time::sleep(ASK_GAP).await;
    }
}

/// One question and one answer over a fresh connection.
///
/// A fresh connection per attempt is the point: a half-open socket from an agent
/// that was not listening yet cannot poison the next attempt.
async fn ask_status(host: &str, port: u16) -> Result<()> {
    let mut stream = TcpStream::connect((host, port)).await?;
    handshake(&mut stream, host, port).await?;
    let ask = br#"{"id":1,"method":"session.status","params":{}}"#;
    stream.write_all(&text_frame(ask)).await?;
    stream.flush().await?;
    let reply = read_text_frame(&mut stream).await?;
    let body = String::from_utf8_lossy(&reply);
    if body.contains("\"ready\"") {
        return Ok(());
    }
    Err(anyhow!("session.status answered without a ready field: {body}"))
}

/// Upgrade the connection, and refuse anything but `101 Switching Protocols`.
///
/// The server's `Sec-WebSocket-Accept` is not checked. This talks to a port the
/// caller just spawned on the loopback interface, so the digest would guard
/// against nothing, and a wrong one still cannot answer a BiDi command.
async fn handshake(stream: &mut TcpStream, host: &str, port: u16) -> Result<()> {
    let key = base64::engine::general_purpose::STANDARD.encode(nonce());
    let request = format!(
        "GET /session HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await? == 0 {
            return Err(anyhow!("the endpoint closed the connection during upgrade"));
        }
        head.push(byte[0]);
        if head.len() > 8 * 1024 {
            return Err(anyhow!("upgrade response headers exceeded 8KiB"));
        }
    }
    let status = String::from_utf8_lossy(&head);
    let first = status.lines().next().unwrap_or_default();
    if !first.contains(" 101") {
        return Err(anyhow!("the endpoint refused the upgrade: {first}"));
    }
    Ok(())
}

/// Sixteen bytes for the upgrade key. The value is never read back, only echoed,
/// so process id and clock are enough to keep two concurrent probes distinct.
fn nonce() -> [u8; 16] {
    let mut out = [0u8; 16];
    let pid = std::process::id().to_le_bytes();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default()
        .to_le_bytes();
    out[..4].copy_from_slice(&pid);
    out[4..12].copy_from_slice(&now);
    out[12..].copy_from_slice(&[0x6c, 0x75, 0x72, 0x69]);
    out
}

/// A client text frame: final, masked, with the shortest length form that fits.
///
/// Masking is mandatory for a client frame and the key may be any four bytes; a
/// server that reads the frame at all unmasks it, so the value carries no meaning
/// beyond making the payload differ on the wire.
fn text_frame(payload: &[u8]) -> Vec<u8> {
    let mask = nonce()[..4].to_vec();
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x81);
    let n = payload.len();
    if n < 126 {
        frame.push(0x80 | n as u8);
    } else if n <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(n as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    frame
}

/// Read frames until a text one arrives, skipping whatever the agent sends first.
///
/// A remote agent is free to send a ping or a close before an answer. Neither is
/// the reply, so both are read past rather than mistaken for one.
async fn read_text_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    loop {
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).await?;
        let opcode = head[0] & 0x0f;
        let masked = head[1] & 0x80 != 0;
        let mut len = usize::from(head[1] & 0x7f);
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).await?;
            len = usize::from(u16::from_be_bytes(ext));
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext).await?;
            len = usize::try_from(u64::from_be_bytes(ext))
                .map_err(|_| anyhow!("frame length does not fit this platform"))?;
        }
        if len > MAX_FRAME {
            return Err(anyhow!("the endpoint sent a {len} byte frame"));
        }
        let mut mask = [0u8; 4];
        if masked {
            stream.read_exact(&mut mask).await?;
        }
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match opcode {
            0x1 => return Ok(payload),
            0x8 => return Err(anyhow!("the endpoint closed the connection")),
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// A server that answers the upgrade and then replies with `body` as one text
    /// frame. Returns the port it listens on.
    async fn serve(body: &'static str, upgrade: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("accept");
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if sock.read(&mut byte).await.expect("read") == 0 {
                    return;
                }
                head.push(byte[0]);
            }
            sock.write_all(upgrade.as_bytes()).await.expect("upgrade");
            if !upgrade.contains(" 101") {
                return;
            }
            let mut frame = [0u8; 2];
            sock.read_exact(&mut frame).await.expect("frame head");
            let len = usize::from(frame[1] & 0x7f);
            let mut rest = vec![0u8; len + 4];
            sock.read_exact(&mut rest).await.expect("frame body");
            let mut out = vec![0x81, body.len() as u8];
            out.extend_from_slice(body.as_bytes());
            sock.write_all(&out).await.expect("reply");
        });
        port
    }

    /// A masked client frame is what a conforming server expects: the mask bit is
    /// set, the length is the payload's, and unmasking restores the bytes.
    #[test]
    fn a_client_frame_is_masked_and_reversible() {
        let frame = text_frame(b"hello");
        assert_eq!(frame[0], 0x81);
        assert_eq!(frame[1], 0x80 | 5);
        let mask = &frame[2..6];
        let body: Vec<u8> = frame[6..]
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        assert_eq!(body, b"hello");
    }

    /// A payload past 125 bytes moves to the two byte length form, which a server
    /// reads differently. Getting this wrong desynchronises the stream instead of
    /// failing, so it is pinned.
    #[test]
    fn a_long_payload_uses_the_extended_length_form() {
        let payload = vec![b'x'; 200];
        let frame = text_frame(&payload);
        assert_eq!(frame[1], 0x80 | 126);
        assert_eq!(u16::from_be_bytes([frame[2], frame[3]]), 200);
        assert_eq!(frame.len(), 4 + 4 + 200);
    }

    #[tokio::test]
    async fn an_endpoint_that_answers_ready_is_ready() {
        let port = serve(
            r#"{"id":1,"type":"success","result":{"ready":true,"message":""}}"#,
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n",
        )
        .await;
        wait_until_ready("127.0.0.1", port, Duration::from_secs(2))
            .await
            .expect("a ready endpoint");
    }

    /// An answer without the field is not an answer. A status reply is the only
    /// evidence the agent's loop is turning, so anything else keeps waiting and
    /// then names what came back.
    #[tokio::test]
    async fn an_answer_without_ready_is_not_readiness() {
        let port = serve(
            r#"{"id":1,"type":"error","error":"unknown command"}"#,
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n",
        )
        .await;
        let err = wait_until_ready("127.0.0.1", port, Duration::from_millis(300))
            .await
            .expect_err("an error reply is not readiness");
        assert!(err.to_string().contains("never answered session.status"), "{err}");
    }

    /// A port bound by something that is not a remote agent must not be reported
    /// as a browser ready to drive.
    #[tokio::test]
    async fn a_port_that_refuses_the_upgrade_is_not_a_browser() {
        let port = serve("", "HTTP/1.1 404 Not Found\r\n\r\n").await;
        let err = wait_until_ready("127.0.0.1", port, Duration::from_millis(300))
            .await
            .expect_err("a 404 is not readiness");
        assert!(err.to_string().contains("session.status"), "{err}");
    }

    /// Nothing listening is the ordinary case while a browser is still starting.
    /// It must report the address, because a wrong port looks identical to a slow
    /// start until the message says which one was asked.
    #[tokio::test]
    async fn a_closed_port_is_named_in_the_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let err = wait_until_ready("127.0.0.1", port, Duration::from_millis(200))
            .await
            .expect_err("a closed port is not readiness");
        assert!(err.to_string().contains(&port.to_string()), "{err}");
    }
}
