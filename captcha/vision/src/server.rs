//! The loopback line server.
//!
//! One connection, one request line, one reply line, close. No keep-alive and no
//! session state: the helper has nothing to remember between crops, and a helper
//! that remembers is a helper that can leak.
//!
//! The listener refuses a non-loopback address. The engine already refuses to talk
//! to one, and a helper reachable from the network is a perception service anyone
//! on the segment can queue work on.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};

use crate::proto::{Reply, Request};

/// Longest request line accepted, which bounds the crop a caller can push.
const MAX_LINE: u64 = 32 * 1024 * 1024;

/// Is this address on the loopback interface?
#[must_use]
pub fn is_loopback(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Bind, refusing anything but loopback.
///
/// # Errors
/// If the address is not loopback, or the port cannot be bound.
pub fn bind(addr: SocketAddr) -> Result<TcpListener, String> {
    if !is_loopback(&addr) {
        return Err(format!(
            "refusing to listen on {addr}: this helper is loopback only"
        ));
    }
    TcpListener::bind(addr).map_err(|e| format!("cannot listen on {addr}: {e}"))
}

/// Answer one connection.
pub fn serve_one(stream: &mut TcpStream) {
    let peer = stream.peer_addr().ok();
    if let Some(peer) = peer {
        if !is_loopback(&peer) {
            // Belt and braces: the listener is loopback, so this cannot normally
            // happen, and if it ever does the crop is not looked at.
            let _ = stream.write_all(b"{\"error\":\"not a loopback peer\"}\n");
            return;
        }
    }
    let read_stream = match stream.try_clone() {
        Ok(clone) => clone,
        Err(e) => {
            let _ = writeln!(stream, "{{\"error\":\"cannot read request: {e}\"}}");
            return;
        }
    };
    let mut line = String::new();
    let mut reader = BufReader::new(read_stream).take(MAX_LINE);
    let reply = match reader.read_line(&mut line) {
        Ok(0) => Reply::refused("empty request"),
        Ok(_) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(request) => crate::answer(&request),
            Err(e) => Reply::refused(format!("request is not this protocol: {e}")),
        },
        Err(e) => Reply::refused(format!("cannot read request: {e}")),
    };
    let body = serde_json::to_string(&reply).unwrap_or_else(|e| {
        format!("{{\"error\":\"cannot serialize reply: {e}\"}}")
    });
    let _ = writeln!(stream, "{body}");
    let _ = stream.flush();
}

/// Serve until the listener is dropped. One connection at a time is enough: a
/// solve asks one question per widget.
pub fn serve(listener: &TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => serve_one(&mut stream),
            Err(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn ask(port: u16, line: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.write_all(line.as_bytes()).expect("write");
        stream.write_all(b"\n").expect("newline");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        out
    }

    fn spawn() -> u16 {
        let listener = bind("127.0.0.1:0".parse().expect("addr")).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || serve(&listener));
        port
    }

    #[test]
    fn a_non_loopback_address_is_refused_at_bind() {
        let err = bind("0.0.0.0:0".parse().expect("addr")).expect_err("must refuse");
        assert!(err.contains("loopback only"), "{err}");
    }

    #[test]
    fn a_line_that_is_not_the_protocol_is_answered_with_an_error_not_a_hang() {
        let port = spawn();
        let reply = ask(port, "hello");
        assert!(reply.contains("not this protocol"), "{reply}");
    }

    #[test]
    fn an_empty_line_is_answered() {
        let port = spawn();
        let reply = ask(port, "");
        assert!(reply.contains("error"), "{reply}");
    }

    #[test]
    fn a_well_formed_request_with_no_image_is_refused() {
        let port = spawn();
        let reply = ask(port, "{\"kind\":\"slider\",\"task\":\"axis\",\"png\":\"\"}");
        assert!(reply.contains("png"), "{reply}");
    }
}
