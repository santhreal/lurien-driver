//! The helper line protocol, and the base64 the engine wraps a PNG in.
//!
//! One JSON object per line in, one out. The request carries a crop and nothing
//! else: no URL, no cookies, no session. That is the whole security argument for
//! running perception outside the browser, and it only holds if the request stays
//! this small.

use serde::{Deserialize, Serialize};

/// Version of this line protocol.
///
/// The helper and the browser ship as separate builds and are wired together by
/// whoever starts them, so a mismatched pair is a normal operational state rather
/// than a bug. A version on every line turns it into one refusal instead of a
/// field-by-field misreading of a request. `HelperSock.sys.mjs` sends this number
/// and the tests hold the two copies equal.
pub const PROTOCOL_VERSION: u64 = 2;

/// What the engine asks for.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// Protocol version the caller speaks. A request that names none is from a
    /// build older than the version itself, which is not this protocol.
    #[serde(default)]
    pub v: u64,
    /// This session's helper token, minted by whoever started both processes.
    #[serde(default)]
    pub token: String,
    /// Challenge kind. `slider` and `visual` are answered here.
    pub kind: String,
    /// Task within the kind: `axis` for a slider, `cells` for a grid.
    pub task: String,
    /// The crop, PNG, base64.
    #[serde(default)]
    pub png: String,
    /// Crop width in CSS pixels, which may differ from the PNG width when the
    /// snapshot was taken at a scale.
    #[serde(default)]
    pub width: f64,
    /// Crop height in CSS pixels.
    #[serde(default)]
    pub height: f64,
    /// What the widget asked for, in the widget's own words. A grid request
    /// without it is refused: the question is the whole task.
    #[serde(default)]
    pub prompt: String,
    /// The cells of the grid, in the crop's own pixels and in the order the
    /// browser laid them out. The answer is indices into this list, so the
    /// browser clicks its own rectangles rather than coordinates from a helper.
    #[serde(default)]
    pub cells: Vec<crate::grid::Cell>,
}

/// What the helper answers. An answer carries one kind's result or an error,
/// never both, and never a silent zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reply {
    /// Slider travel along x, in the caller's own pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,
    /// Slider travel along y, which is zero for every slider seen so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,
    /// How sure the measurement or the weakest chosen cell is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Indices into the request's cell list, in grid order. An empty list is an
    /// answer: nothing in the grid matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cells: Option<Vec<usize>>,
    /// The strongest box in every cell, so a caller can see what a refusal or a
    /// thin margin was made of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scores: Option<Vec<f32>>,
    /// Why there is no answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    /// A measured slider axis.
    #[must_use]
    pub fn axis(dx: f64, confidence: f32) -> Self {
        Self {
            dx: Some(dx),
            dy: Some(0.0),
            confidence: Some(confidence),
            cells: None,
            scores: None,
            error: None,
        }
    }

    /// The cells that answer a grid, with every cell's score.
    #[must_use]
    pub fn grid(cells: Vec<usize>, scores: Vec<f32>) -> Self {
        let confidence = cells
            .iter()
            .filter_map(|index| scores.get(*index).copied())
            .fold(f32::INFINITY, f32::min);
        Self {
            dx: None,
            dy: None,
            confidence: confidence.is_finite().then_some(confidence),
            cells: Some(cells),
            scores: Some(scores),
            error: None,
        }
    }

    /// No answer, and why.
    #[must_use]
    pub fn refused(reason: impl Into<String>) -> Self {
        Self {
            dx: None,
            dy: None,
            confidence: None,
            cells: None,
            scores: None,
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
