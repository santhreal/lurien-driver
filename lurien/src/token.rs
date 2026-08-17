//! Session tokens for the driver's local channels.
//!
//! Two loopback channels carry this session's data: the engine's control socket
//! and the perception helper. Loopback is not access control, because every
//! process on the host can connect to both, so each is private only for as long as
//! the token is unguessable. One minter, so neither channel can quietly grow a
//! weaker one.

/// A token no other process on this host can guess: 24 bytes of entropy as hex.
///
/// Drawn from the OS entropy pool, never from a seed a caller can name. The
/// dynamics seed is reproducible on purpose; a credential never is.
#[must_use]
pub fn session_token() -> String {
    use rand::{Rng, SeedableRng};
    use std::fmt::Write as _;
    let mut rng = rand::rngs::StdRng::from_entropy();
    let mut hex = String::with_capacity(48);
    for _ in 0..24 {
        let _ = write!(hex, "{:02x}", rng.gen::<u8>());
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token is the whole access control of a loopback channel. A constant, a
    /// short value, or anything derived from the clock would be guessable by the
    /// other processes on this host, which is exactly who the channel is closed to.
    #[test]
    fn two_tokens_never_match_and_each_is_full_length() {
        let one = session_token();
        let two = session_token();
        assert_ne!(one, two, "two sessions were minted the same token");
        assert_eq!(one.len(), 48, "a token is 24 bytes of hex");
        assert!(
            one.chars().all(|c| c.is_ascii_hexdigit()),
            "a token holds something other than hex: {one}"
        );
    }
}
