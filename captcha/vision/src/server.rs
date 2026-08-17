//! The loopback line server.
//!
//! One connection, one request line, one reply line, close. No keep-alive and no
//! session state: the helper has nothing to remember between crops, and a helper
//! that remembers is a helper that can leak.
//!
//! The listener refuses a non-loopback address, and every request must name this
//! protocol version and this session's token. Loopback is not access control: any
//! process on the host can reach the port, including whatever the page just got
//! the browser to run, and an unauthenticated helper is a perception service they
//! can queue work on and read answers from.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};

use crate::proto::{Reply, Request, PROTOCOL_VERSION};

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

/// Is this the token the helper was started with?
///
/// Compared in constant time. The reply is one line on loopback, so a caller can
/// time thousands of attempts a second, and a length-then-prefix comparison hands
/// the token over a byte at a time.
#[must_use]
pub fn token_ok(expected: &str, given: &str) -> bool {
    if expected.is_empty() || expected.len() != given.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.bytes().zip(given.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Answer one connection, refusing anything that is not this protocol version
/// carrying this session's token.
pub fn serve_one(stream: &mut TcpStream, token: &str) {
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
            Ok(request) => authorized(&request, token),
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

/// The answer for a request that proved which protocol it speaks and which
/// session it belongs to. Loopback is not access control: every process on this
/// host, including a page's own helper, can reach the port.
fn authorized(request: &Request, token: &str) -> Reply {
    if request.v != PROTOCOL_VERSION {
        return Reply::refused(format!(
            "this helper speaks protocol {PROTOCOL_VERSION} and the request names {}; \
             run the helper and the browser from one build",
            request.v
        ));
    }
    if token.is_empty() {
        return Reply::refused(
            "this helper was started with no session token, so it answers nothing; \
             pass --token, or LURIEN_HELPER_TOKEN, the same value the session names",
        );
    }
    if !token_ok(token, &request.token) {
        return Reply::refused(
            "the token does not match this helper's session; \
             name the same token in the session's helper configuration",
        );
    }
    crate::answer(request)
}

/// Serve until the listener is dropped. One connection at a time is enough: a
/// solve asks one question per widget.
pub fn serve(listener: &TcpListener, token: &str) {
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => serve_one(&mut stream, token),
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

    const TOKEN: &str = "6f1c9a4b7d2e08135a6c4f9e2b7d10c3";

    fn spawn_with(token: &'static str) -> u16 {
        let listener = bind("127.0.0.1:0".parse().expect("addr")).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || serve(&listener, token));
        port
    }

    fn spawn() -> u16 {
        spawn_with(TOKEN)
    }

    /// A request as the engine sends it: version, token, then the crop.
    fn line(body: &str) -> String {
        format!("{{\"v\":{PROTOCOL_VERSION},\"token\":\"{TOKEN}\",{body}}}")
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
        let reply = ask(port, &line("\"kind\":\"slider\",\"task\":\"axis\",\"png\":\"\""));
        assert!(reply.contains("png"), "{reply}");
    }

    /// Loopback lets every process on the host queue work here, so the token is
    /// the whole access control. Each way of arriving without it is checked, not
    /// only the one somebody thought of: no token, an empty token, a token of the
    /// right length that differs, and the right token on the wrong protocol.
    #[test]
    fn a_request_without_this_session_token_is_refused() {
        let port = spawn();
        let crop = "\"kind\":\"slider\",\"task\":\"axis\",\"png\":\"\"";
        let mut wrong: String = TOKEN.to_string();
        wrong.replace_range(0..1, "0");
        assert_eq!(wrong.len(), TOKEN.len(), "the wrong token must be the same length");
        let unauthenticated = [
            format!("{{\"v\":{PROTOCOL_VERSION},{crop}}}"),
            format!("{{\"v\":{PROTOCOL_VERSION},\"token\":\"\",{crop}}}"),
            format!("{{\"v\":{PROTOCOL_VERSION},\"token\":\"{wrong}\",{crop}}}"),
            format!("{{\"v\":{PROTOCOL_VERSION},\"token\":\"{TOKEN}extra\",{crop}}}"),
        ];
        for request in &unauthenticated {
            let reply = ask(port, request);
            assert!(
                reply.contains("the token does not match"),
                "an unauthenticated request was answered: {request} -> {reply}"
            );
            assert!(
                !reply.contains("dx"),
                "the helper answered an unauthenticated request with a measurement: {reply}"
            );
        }
        // The right token on another protocol version is refused for the version,
        // before anything reads the crop.
        let reply = ask(port, &format!("{{\"v\":99,\"token\":\"{TOKEN}\",{crop}}}"));
        assert!(reply.contains("speaks protocol 1"), "{reply}");
    }

    /// A helper started with no token is not an open helper. Every request is
    /// refused, and the refusal says how to start it properly.
    #[test]
    fn a_helper_with_no_token_answers_nothing() {
        let port = spawn_with("");
        let reply = ask(port, &line("\"kind\":\"slider\",\"task\":\"axis\",\"png\":\"\""));
        assert!(reply.contains("no session token"), "{reply}");
    }

    #[test]
    fn a_token_comparison_does_not_accept_a_prefix_or_an_empty_expectation() {
        assert!(token_ok(TOKEN, TOKEN));
        assert!(!token_ok(TOKEN, &TOKEN[..8]));
        assert!(!token_ok(TOKEN, ""));
        assert!(!token_ok("", ""));
        assert!(!token_ok("", TOKEN));
    }

    /// The engine sends the version this crate defines. Two constants in two
    /// repositories, so a bump on one side alone must be red rather than a run-time
    /// refusal against a helper that is in fact current.
    #[test]
    fn the_engine_client_speaks_this_protocol_version() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../engine/additions/challenge/HelperSock.sys.mjs");
        let Ok(source) = std::fs::read_to_string(&path) else {
            // The browser is a sibling checkout. Say so, so a skip is not read
            // as a protocol version that agreed.
            println!("SKIP: no browser checkout at {}", path.display());
            return;
        };
        let needle = "const PROTOCOL_VERSION = ";
        let index = source.find(needle).expect("HelperSock declares PROTOCOL_VERSION");
        let spoken: u64 = source[index + needle.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("PROTOCOL_VERSION is a number");
        assert_eq!(
            spoken, PROTOCOL_VERSION,
            "the engine sends protocol {spoken} and this helper speaks {PROTOCOL_VERSION}"
        );
    }
}
