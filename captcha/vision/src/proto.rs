//! The helper line protocol, and the base64 the engine wraps a PNG in.
//!
//! One JSON object per line in, one out. The request carries a crop and nothing
//! else: no URL, no cookies, no session. That is the whole security argument for
//! running perception outside the browser, and it only holds if the request stays
//! this small.

use serde::{Deserialize, Serialize};

/// What the engine asks for.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// Challenge kind. Only `slider` is answered here.
    pub kind: String,
    /// Task within the kind. Only `axis` is answered here.
    pub task: String,
    /// The crop, PNG, base64.
    #[serde(default)]
    pub png: String,
    /// Crop width in CSS pixels, which may differ from the PNG width when the
    /// snapshot was taken at a scale.
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

/// What the helper answers. An answer carries either an axis or an error, never
/// both, and never a silent zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    #[must_use]
    pub fn axis(dx: f64, confidence: f32) -> Self {
        Self {
            dx: Some(dx),
            dy: Some(0.0),
            confidence: Some(confidence),
            error: None,
        }
    }

    #[must_use]
    pub fn refused(reason: impl Into<String>) -> Self {
        Self {
            dx: None,
            dy: None,
            confidence: None,
            error: Some(reason.into()),
        }
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn value(byte: u8) -> Option<u8> {
    ALPHABET
        .iter()
        .position(|c| *c == byte)
        .map(|v| u8::try_from(v).expect("alphabet index fits"))
}

/// Decode standard base64, ignoring line breaks and accepting a missing tail pad.
///
/// # Errors
/// If the input holds a character outside the alphabet, or ends mid-quantum with
/// a single leftover character, which cannot encode a byte.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut quantum = [0u8; 4];
    let mut filled = 0usize;
    for byte in text.bytes() {
        match byte {
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            b'=' => break,
            _ => {}
        }
        let Some(bits) = value(byte) else {
            return Err(format!("not base64: byte {byte:#04x}"));
        };
        quantum[filled] = bits;
        filled += 1;
        if filled == 4 {
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
            out.push((quantum[2] << 6) | quantum[3]);
            filled = 0;
        }
    }
    match filled {
        0 => {}
        1 => return Err("base64 ends with a single leftover character".to_string()),
        2 => out.push((quantum[0] << 2) | (quantum[1] >> 4)),
        _ => {
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            out.push((quantum[1] << 4) | (quantum[2] >> 2));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(bytes: &[u8]) -> String {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let bits = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    let index = usize::try_from((bits >> (18 - 6 * i)) & 0x3f).expect("6 bits");
                    out.push(char::from(ALPHABET[index]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn every_payload_length_round_trips() {
        for len in 0..64usize {
            let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i * 7 % 251).expect("in range")).collect();
            let decoded = base64_decode(&encode(&bytes)).expect("round trip");
            assert_eq!(decoded, bytes, "length {len} did not survive the round trip");
        }
    }

    #[test]
    fn line_breaks_are_ignored() {
        let bytes = b"a png would be much longer than this";
        let wrapped = encode(bytes)
            .as_bytes()
            .chunks(8)
            .map(|c| String::from_utf8(c.to_vec()).expect("ascii"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(base64_decode(&wrapped).expect("wrapped"), bytes);
    }

    #[test]
    fn a_character_outside_the_alphabet_is_an_error() {
        assert!(base64_decode("AAA$").is_err());
    }

    #[test]
    fn a_single_leftover_character_is_an_error() {
        // One base64 character carries six bits, which cannot be a byte. Accepting
        // it would silently drop the tail of a truncated PNG.
        assert!(base64_decode("AAAAA").is_err());
    }

    #[test]
    fn a_reply_carries_an_axis_or_an_error_but_never_both() {
        let axis = serde_json::to_string(&Reply::axis(42.0, 6.5)).expect("json");
        assert!(axis.contains("\"dx\":42.0"), "{axis}");
        assert!(!axis.contains("error"), "{axis}");
        let refused = serde_json::to_string(&Reply::refused("no notch")).expect("json");
        assert!(refused.contains("no notch"), "{refused}");
        assert!(!refused.contains("dx"), "{refused}");
    }
}
