//! The privileged channel into the engine.
//!
//! Some state is not reachable from outside a browser. A device position is the
//! plain example: it lives in the geolocation service of the process that owns
//! the tab, so no page script, no pref and no network provider moves a page that
//! has already read a fix. The engine therefore opens one loopback line
//! protocol per session and this is the client for it.
//!
//! Configuration goes out in the environment at launch, which is why the port is
//! chosen here rather than reported back: the engine binds the port it was given
//! and a verb can then reach it without a handshake. The token is what keeps the
//! open port private to this session, since anything else on the host could
//! connect to loopback.

use crate::error::Error;
use crate::geo::Position;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Environment variable the engine reads its control configuration from.
pub const CONTROL_ENV: &str = "LURIEN_CONTROL";

/// How long a control call may take. The engine answers from the parent process
/// without touching the network, so anything slower than this is a browser that
/// is gone rather than a browser that is busy.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Longest reply line accepted, so a wrong service on the port cannot stream.
const MAX_REPLY: u64 = 64 * 1024;

/// The control channel of one session.
#[derive(Debug, Clone)]
pub struct Control {
    port: u16,
    token: String,
}

impl Control {
    /// Choose a port and a token for a session that has not launched yet.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when loopback cannot be probed for a free
    /// port.
    pub fn reserve() -> Result<Self, Error> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| {
            Error::ControlUnavailable {
                detail: format!("cannot reserve a control port on loopback: {e}. Check that 127.0.0.1 is reachable"),
            }
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::ControlUnavailable {
                detail: format!("the reserved control port has no address: {e}"),
            })?
            .port();
        // Released here so the engine can bind it. Nothing else on this host is
        // handed the port, and the token is what makes the channel private.
        drop(listener);
        Ok(Self {
            port,
            token: token(),
        })
    }

    /// The port the engine was told to listen on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The value of [`CONTROL_ENV`] for a launch that starts at `position`.
    #[must_use]
    pub fn env_value(&self, position: Option<Position>) -> String {
        let mut json = format!(
            "{{\"port\":{},\"token\":\"{}\"",
            self.port,
            self.token.escape_default()
        );
        if let Some(p) = position {
            json.push_str(&format!(
                ",\"position\":{{\"latitude\":{},\"longitude\":{},\"accuracy\":{}}}",
                p.latitude, p.longitude, p.accuracy_m
            ));
        }
        json.push('}');
        json
    }

    /// The environment entry for a launch that starts at `position`.
    #[must_use]
    pub fn env_entry(&self, position: Option<Position>) -> (String, String) {
        (CONTROL_ENV.to_string(), self.env_value(position))
    }

    /// Serve `position` to every page of this session, loaded or not yet.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine is not reachable or refuses
    /// the request.
    pub async fn set_position(&self, position: Position) -> Result<(), Error> {
        let params = format!(
            "{{\"latitude\":{},\"longitude\":{},\"accuracy\":{}}}",
            position.latitude, position.longitude, position.accuracy_m
        );
        self.call("position.set", &params).await.map(|_| ())
    }

    /// Stop serving a position, so pages fall back to what the browser has.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when the engine is not reachable or refuses
    /// the request.
    pub async fn clear_position(&self) -> Result<(), Error> {
        self.call("position.clear", "{}").await.map(|_| ())
    }

    /// Whether the engine is listening and accepts this session's token.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnavailable`] when it is not.
    pub async fn ping(&self) -> Result<(), Error> {
        self.call("ping", "{}").await.map(|_| ())
    }

    /// One request, one reply, connection closed.
    async fn call(&self, op: &str, params: &str) -> Result<String, Error> {
        let unreachable = |detail: String| Error::ControlUnavailable { detail };
        let line = format!(
            "{{\"token\":\"{}\",\"op\":\"{op}\",\"params\":{params}}}\n",
            self.token.escape_default()
        );
        let work = async {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await?;
            stream.write_all(line.as_bytes()).await?;
            stream.flush().await?;
            let mut reply = String::new();
            // Bounded: a wrong service on this port must not be able to stream.
            let mut reader = BufReader::new(AsyncReadExt::take(&mut stream, MAX_REPLY));
            reader.read_line(&mut reply).await?;
            Ok::<String, std::io::Error>(reply)
        };
        let reply = match tokio::time::timeout(CALL_TIMEOUT, work).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(e)) => {
                return Err(unreachable(format!(
                    "the engine control channel on port {} did not answer {op}: {e}. Check that the session is still running",
                    self.port
                )));
            }
            Err(_) => {
                return Err(unreachable(format!(
                    "the engine control channel on port {} went quiet during {op}. Check that the session is still running",
                    self.port
                )));
            }
        };
        parse_reply(&reply, op, self.port)
    }
}

/// Read one reply line: refuse anything that is not this protocol answering yes.
fn parse_reply(reply: &str, op: &str, port: u16) -> Result<String, Error> {
    let unreachable = |detail: String| Error::ControlUnavailable { detail };
    let trimmed = reply.trim();
    if trimmed.is_empty() {
        return Err(unreachable(format!(
            "the engine control channel on port {port} closed without answering {op}. Check that the session is still running"
        )));
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|_| {
        unreachable(format!(
            "the reply to {op} on port {port} is not this protocol: {trimmed:?}. Check that nothing else took the port"
        ))
    })?;
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(trimmed.to_string());
    }
    let detail = value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("the engine gave no reason");
    Err(unreachable(format!("the engine refused {op}: {detail}")))
}

/// A token no other process on this host can guess.
fn token() -> String {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::from_entropy();
    let mut hex = String::with_capacity(48);
    for _ in 0..24 {
        hex.push_str(&format!("{:02x}", rng.gen::<u8>()));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port that is handed to the engine and a token that is not guessable are
    /// the whole access control of this channel, so neither may be constant.
    #[test]
    fn two_sessions_never_share_a_token_or_a_port() {
        let one = Control::reserve().expect("reserve one");
        let two = Control::reserve().expect("reserve two");
        assert_ne!(one.token, two.token, "two sessions share a control token");
        assert_ne!(one.port(), two.port(), "two sessions share a control port");
        assert_eq!(one.token.len(), 48, "a token is 24 bytes of hex");
    }

    /// The engine reads this with a JSON parser and no defaults: a launch
    /// position has to arrive as three numbers under `position`, and a session
    /// with no position must not send the key at all.
    #[test]
    fn the_launch_value_carries_the_port_the_token_and_the_position() {
        let control = Control::reserve().expect("reserve");
        let bare: serde_json::Value =
            serde_json::from_str(&control.env_value(None)).expect("bare value parses");
        assert_eq!(bare["port"], control.port());
        assert_eq!(bare["token"], control.token);
        assert!(bare.get("position").is_none(), "a bare launch names a position");

        let position = Position::new(52.52, 13.405, 55.0).expect("position");
        let with: serde_json::Value = serde_json::from_str(&control.env_value(Some(position)))
            .expect("value with a position parses");
        assert_eq!(with["position"]["latitude"], 52.52);
        assert_eq!(with["position"]["longitude"], 13.405);
        assert_eq!(with["position"]["accuracy"], 55.0);
        assert_eq!(control.env_entry(Some(position)).0, CONTROL_ENV);
    }

    /// Every failure of this channel has to name the port and stay a
    /// [`Error::ControlUnavailable`], because the verb above turns it into the
    /// only thing the caller can act on.
    #[test]
    fn a_reply_that_is_not_this_protocol_is_refused() {
        let cases = [
            ("", "closed without answering"),
            ("not json\n", "is not this protocol"),
            ("{\"ok\":false,\"error\":\"the token does not match this session\"}\n",
             "the token does not match"),
            ("{\"result\":1}\n", "the engine gave no reason"),
        ];
        for (reply, want) in cases {
            let err = parse_reply(reply, "position.set", 4242).expect_err("must refuse");
            let text = err.to_string();
            assert!(
                text.contains(want),
                "reply {reply:?} produced {text:?}, which does not mention {want:?}"
            );
            assert!(
                matches!(err, Error::ControlUnavailable { .. }),
                "reply {reply:?} produced {err:?} instead of a control error"
            );
        }
        let ok = parse_reply("{\"ok\":true,\"applied\":1}\n", "position.set", 4242)
            .expect("a success reply is accepted");
        assert!(ok.contains("\"applied\":1"));
    }

    /// A live engine is not needed to prove the client refuses a dead port: the
    /// error has to arrive as a control error naming the port, not as a hang.
    #[tokio::test]
    async fn a_call_to_a_port_nobody_holds_names_that_port() {
        let control = Control::reserve().expect("reserve");
        let err = control.ping().await.expect_err("nothing is listening");
        assert!(
            err.to_string().contains(&control.port().to_string()),
            "the error {err} does not name port {}",
            control.port()
        );
    }
}
