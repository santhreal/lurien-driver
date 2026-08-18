//! The perception helper: one crop or one clip in, one answer out.
//!
//! The engine snapshots the widget's own browsing context, or fetches the clip the
//! widget itself was told to play, and sends it here. This process sees a few
//! hundred pixels or a few seconds of sound and nothing else, which is why it is a
//! process: perception does not belong in libxul, and a model that cannot see a
//! session cannot leak one. It has no network, so a challenge is never fetched
//! twice from two clients.
//!
//! Three kinds are answered. A slider is measured, which is arithmetic over a crop
//! and needs no weights. A grid is detected and a clip is transcribed, which each
//! need a model, so a helper started without one refuses those requests by name and
//! still measures sliders.

pub mod asr;
pub mod detect;
pub mod gap;
pub mod grid;
pub mod pixels;
pub mod proto;
pub mod server;
pub mod sound;

use std::path::{Path, PathBuf};

use gap::Gray;

/// Decode a PNG crop to luminance.
///
/// # Errors
/// If the bytes are not a PNG this build can read, or the frame is empty.
pub fn decode_png(bytes: &[u8]) -> Result<Gray, String> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().map_err(|e| format!("not a png: {e}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("unreadable png: {e}"))?;
    let width = usize::try_from(frame.width).map_err(|_| "png too wide".to_string())?;
    let height = usize::try_from(frame.height).map_err(|_| "png too tall".to_string())?;
    if width == 0 || height == 0 {
        return Err("png has no pixels".to_string());
    }
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => return Err(format!("unsupported png color type {other:?}")),
    };
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(format!("unsupported png bit depth {:?}", frame.bit_depth));
    }
    let data = &buffer[..frame.buffer_size()];
    let mut pixels = Vec::with_capacity(width * height);
    for chunk in data.chunks_exact(channels) {
        // Rec. 601 luma. Edge energy is what matters here, so any sane weighting
        // works, but a fixed one keeps the answer reproducible.
        let value = match channels {
            1 | 2 => f32::from(chunk[0]),
            _ => {
                0.299 * f32::from(chunk[0]) + 0.587 * f32::from(chunk[1]) + 0.114 * f32::from(chunk[2])
            }
        };
        pixels.push(value.round().clamp(0.0, 255.0) as u8);
    }
    if pixels.len() != width * height {
        return Err("png rows do not fill the frame".to_string());
    }
    Ok(Gray::new(width, height, pixels))
}

/// Pixels a cell may hang over the edge of the crop before it is refused.
///
/// A rectangle measured in CSS pixels and a crop taken at a device scale disagree
/// by rounding on every side. One pixel of slack is that rounding; more than that
/// is a widget that was not fully on screen.
const EDGE: i64 = 1;

/// The helper's state: the models, once something asks for one.
///
/// Weights are hundreds of megabytes and take seconds to open, so each is loaded on
/// the first request that needs it and kept. A session that only ever measures
/// sliders never pays for either, a session that only solves grids never opens a
/// speech model, and a session whose model directory is wrong finds out on the
/// request that needed it, with the path in the refusal.
pub struct Helper {
    model_dir: Option<PathBuf>,
    detector: Option<detect::Detector>,
    /// The load failure, kept so a second request does not spend another few
    /// seconds proving the same directory is still not a model.
    refused: Option<String>,
    audio_dir: Option<PathBuf>,
    listener: Option<asr::Listener>,
    deaf: Option<String>,
}

impl Helper {
    /// A helper that will look for a detector in `model_dir` when a grid arrives and
    /// for a speech model in `audio_dir` when a clip does.
    #[must_use]
    pub fn new(model_dir: Option<PathBuf>, audio_dir: Option<PathBuf>) -> Self {
        Self {
            model_dir,
            detector: None,
            refused: None,
            audio_dir,
            listener: None,
            deaf: None,
        }
    }

    /// The detector directory this helper was given, if any.
    #[must_use]
    pub fn model_dir(&self) -> Option<&Path> {
        self.model_dir.as_deref()
    }

    /// The speech model directory this helper was given, if any.
    #[must_use]
    pub fn audio_dir(&self) -> Option<&Path> {
        self.audio_dir.as_deref()
    }

    /// Answer one request.
    #[must_use]
    pub fn answer(&mut self, request: &proto::Request) -> proto::Reply {
        match (request.kind.as_str(), request.task.as_str()) {
            ("slider", "axis") => slider(request),
            ("visual", "cells") => self.cells(request),
            ("audio", "transcribe") => self.transcribe(request),
            ("slider" | "visual" | "audio", task) => proto::Reply::refused(format!(
                "unknown task {task} for kind {}; slider takes axis, visual takes cells \
                 and audio takes transcribe",
                request.kind
            )),
            (kind, _) => proto::Reply::refused(format!(
                "this helper answers the slider, visual and audio kinds, not {kind}"
            )),
        }
    }

    /// What a clip says, in the alphabet the binding named.
    fn transcribe(&mut self, request: &proto::Request) -> proto::Reply {
        if request.audio.is_empty() {
            return proto::Reply::refused(
                "the request carries no clip; the widget's own context owns the audio and \
                 has to fetch the bytes it was told to play",
            );
        }
        let bytes = match proto::base64_decode(&request.audio) {
            Ok(bytes) => bytes,
            Err(e) => return proto::Reply::refused(e),
        };
        // The container is read before a model is opened: bytes that are a page
        // rather than a clip are a binding or a session problem, and proving that
        // costs no weights.
        let clip = match sound::decode(&bytes, &request.mime) {
            Ok(clip) => clip,
            Err(e) => return proto::Reply::refused(e),
        };
        let samples = sound::at_model_rate(&clip);
        let listener = match self.listener() {
            Ok(listener) => listener,
            Err(e) => return proto::Reply::refused(e),
        };
        match listener.hear(&samples, &request.alphabet) {
            Ok(heard) if heard.text.is_empty() => proto::Reply::refused(format!(
                "no reading of the clip spelled anything in {:?}: readings {:?}",
                request.alphabet, heard.heard
            )),
            Ok(heard) => proto::Reply::transcript(heard),
            Err(e) => proto::Reply::refused(e),
        }
    }

    /// Which cells of a grid answer the widget's question.
    fn cells(&mut self, request: &proto::Request) -> proto::Reply {
        let phrase = grid::phrase(&request.prompt);
        if phrase.is_empty() {
            return proto::Reply::refused(format!(
                "the prompt {:?} names nothing to look for, so no cell can be judged; \
                 the binding's prompt selector is reading the wrong element",
                request.prompt
            ));
        }
        if request.cells.is_empty() {
            return proto::Reply::refused(
                "the request carries no cells; the browser owns the grid geometry and \
                 has to send the rectangles it laid out",
            );
        }
        let bytes = match proto::base64_decode(&request.png) {
            Ok(bytes) => bytes,
            Err(e) => return proto::Reply::refused(e),
        };
        let image = match pixels::decode_png_rgb(&bytes) {
            Ok(image) => image,
            Err(e) => return proto::Reply::refused(e),
        };
        // Cells are stated in the caller's pixels; the PNG may have been taken at
        // a scale. Reading a box's cell by CSS pixels on a scaled snapshot puts it
        // in a corner of the wrong cell, which is a wrong answer rather than an
        // error.
        let scale = if request.width > 0.0 {
            image.width() as f64 / request.width
        } else {
            1.0
        };
        // Every rectangle is checked before a model is opened: a request that names
        // a cell the picture does not hold is malformed, and proving that costs no
        // weights.
        let mut cells = Vec::with_capacity(request.cells.len());
        for (index, cell) in request.cells.iter().enumerate() {
            let cell = cell.scaled(scale);
            let x = cell.x.round() as i64;
            let y = cell.y.round() as i64;
            let w = cell.w.round() as i64;
            let h = cell.h.round() as i64;
            // A cell that reaches past the crop would be answered from whatever
            // pixels are inside it, which is a confident answer about a different
            // tile. The caller sent rectangles for a widget that was partly off
            // screen, and that is a refusal, not a guess.
            let over = x < -EDGE
                || y < -EDGE
                || x + w > image.width() as i64 + EDGE
                || y + h > image.height() as i64 + EDGE;
            if over {
                return proto::Reply::refused(format!(
                    "cell {index} at {x},{y} {w}x{h} reaches past the {}x{} crop; \
                     the widget was not fully in the picture that was sent",
                    image.width(),
                    image.height()
                ));
            }
            cells.push(cell);
        }
        let detector = match self.detector() {
            Ok(detector) => detector,
            Err(e) => return proto::Reply::refused(e),
        };
        // One pass over the whole grid, not one per cell. The detector proposes
        // boxes anywhere in the crop and scores each against the phrase, so a small
        // object in a busy tile is a box instead of a diluted caption, and nine
        // tiles cost one model run instead of nine.
        let detections = match detector.detect(&image, &[phrase], grid::REPORT_MIN) {
            Ok(found) => found,
            Err(e) => return proto::Reply::refused(e),
        };
        let scores = grid::cell_scores(&cells, &detections);
        let chosen = grid::chosen(&scores);
        proto::Reply::grid(chosen, scores)
    }

    /// The detector, loading it the first time it is needed.
    fn detector(&mut self) -> Result<&mut detect::Detector, String> {
        if let Some(reason) = &self.refused {
            return Err(reason.clone());
        }
        if self.detector.is_none() {
            let Some(dir) = self.model_dir.clone() else {
                let reason = format!(
                    "this helper was started without a grid classifier, so a grid is refused \
                     rather than guessed; pass --model DIR or set {}",
                    detect::MODEL_ENV
                );
                self.refused = Some(reason.clone());
                return Err(reason);
            };
            match detect::Detector::load(&dir) {
                Ok(model) => self.detector = Some(model),
                Err(e) => {
                    self.refused = Some(e.clone());
                    return Err(e);
                }
            }
        }
        Ok(self.detector.as_mut().expect("just loaded"))
    }

    /// The speech model, loading it the first time it is needed.
    fn listener(&mut self) -> Result<&mut asr::Listener, String> {
        if let Some(reason) = &self.deaf {
            return Err(reason.clone());
        }
        if self.listener.is_none() {
            let Some(dir) = self.audio_dir.clone() else {
                let reason = format!(
                    "this helper was started without a speech model, so a clip is refused \
                     rather than guessed; pass --audio DIR or set {}",
                    asr::MODEL_ENV
                );
                self.deaf = Some(reason.clone());
                return Err(reason);
            };
            match asr::Listener::load(&dir) {
                Ok(model) => self.listener = Some(model),
                Err(e) => {
                    self.deaf = Some(e.clone());
                    return Err(e);
                }
            }
        }
        Ok(self.listener.as_mut().expect("just loaded"))
    }
}

/// Measure a slider crop.
///
/// The travel is reported in the caller's own coordinates: the crop may have been
/// snapshotted at a device scale, so pixels are converted back by the ratio
/// between the stated width and the decoded width. Dragging by device pixels on a
/// scaled display is the classic off-by-a-factor miss.
#[must_use]
fn slider(request: &proto::Request) -> proto::Reply {
    let bytes = match proto::base64_decode(&request.png) {
        Ok(bytes) => bytes,
        Err(e) => return proto::Reply::refused(e),
    };
    let image = match decode_png(&bytes) {
        Ok(image) => image,
        Err(e) => return proto::Reply::refused(e),
    };
    let Some(found) = gap::find(&image) else {
        return proto::Reply::refused("no notch stands out from the background in this crop");
    };
    let scale = if request.width > 0.0 {
        request.width / image.width as f64
    } else {
        1.0
    };
    proto::Reply::axis(found.dx as f64 * scale, found.confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a gray frame as a PNG the way a test can read back.
    fn png_bytes(image: &Gray) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(
                &mut out,
                u32::try_from(image.width).expect("width"),
                u32::try_from(image.height).expect("height"),
            );
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&image.pixels).expect("pixels");
        }
        out
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
                    out.push(char::from(A[usize::try_from((bits >> (18 - 6 * i)) & 0x3f).expect("6 bits")]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn crop(width: usize, height: usize, piece_x: usize, gap_x: usize) -> Gray {
        let mut pixels = vec![210u8; width * height];
        for y in height / 4..height * 3 / 4 {
            for x in piece_x..piece_x + 28 {
                pixels[y * width + x] = 30;
            }
            for x in gap_x..gap_x + 28 {
                pixels[y * width + x] = 70;
            }
        }
        Gray::new(width, height, pixels)
    }

    /// A helper with no model: every test here measures a slider, which needs no
    /// weights, and the grid refusals are about what happens without them.
    fn helper() -> Helper {
        Helper::new(None, None)
    }

    fn request(image: &Gray, width: f64) -> proto::Request {
        serde_json::from_value(serde_json::json!({
            "kind": "slider",
            "task": "axis",
            "png": base64_encode(&png_bytes(image)),
            "width": width,
            "height": image.height as f64,
        }))
        .expect("request")
    }

    /// A grid request over a plain crop, with the cells the caller names.
    fn grid_request(prompt: &str, cells: serde_json::Value) -> proto::Request {
        let image = Gray::new(120, 120, vec![200; 120 * 120]);
        serde_json::from_value(serde_json::json!({
            "kind": "visual",
            "task": "cells",
            "png": base64_encode(&png_bytes(&image)),
            "width": 120.0,
            "height": 120.0,
            "prompt": prompt,
            "cells": cells,
        }))
        .expect("request")
    }

    #[test]
    fn a_cell_that_reaches_past_the_crop_is_refused_before_a_model_is_needed() {
        // The rectangle names pixels the picture does not hold, which happens when
        // the widget was partly off screen. Cropping to what is there would answer
        // about a neighbouring tile, and the refusal has to say so without the cost
        // of opening a model.
        let request = grid_request(
            "Select all images with a bicycle",
            serde_json::json!([{ "x": 60.0, "y": 60.0, "w": 80.0, "h": 80.0 }]),
        );
        let error = helper().answer(&request).error.expect("a refusal");
        assert!(error.contains("reaches past"), "{error}");
        assert!(error.contains("120x120"), "the refusal does not name the crop: {error}");
        assert!(!error.contains("--model"), "the model was opened for a malformed request: {error}");
    }

    #[test]
    fn a_grid_with_a_readable_request_and_no_model_names_the_missing_model() {
        let request = grid_request(
            "Select all images with a bicycle",
            serde_json::json!([{ "x": 0.0, "y": 0.0, "w": 60.0, "h": 60.0 }]),
        );
        let error = helper().answer(&request).error.expect("a refusal");
        assert!(error.contains("--model"), "{error}");
        assert!(error.contains(detect::MODEL_ENV), "{error}");
    }

    #[test]
    fn a_prompt_that_asks_for_nothing_is_refused_by_the_prompt() {
        let request = grid_request("the", serde_json::json!([{ "x": 0.0, "y": 0.0, "w": 10.0, "h": 10.0 }]));
        let error = helper().answer(&request).error.expect("a refusal");
        assert!(error.contains("prompt"), "{error}");
    }

    #[test]
    fn a_grid_with_no_cells_is_refused_rather_than_answered_empty() {
        let request = grid_request("Select all images with a bicycle", serde_json::json!([]));
        let error = helper().answer(&request).error.expect("a refusal");
        assert!(error.contains("no cells"), "{error}");
    }


    #[test]
    fn a_png_crop_round_trips_into_an_axis() {
        let image = crop(300, 65, 12, 196);
        let reply = helper().answer(&request(&image, 300.0));
        let dx = reply.dx.expect("an axis for a crop with a notch");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx} for a travel of 184");
        assert_eq!(reply.error, None);
    }

    #[test]
    fn a_scaled_snapshot_is_reported_in_css_pixels() {
        // Snapshot taken at 2x: the crop is 600 device pixels wide but the widget
        // is 300 CSS pixels, and the drag happens in CSS pixels.
        let image = crop(600, 130, 24, 392);
        let reply = helper().answer(&request(&image, 300.0));
        let dx = reply.dx.expect("an axis");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx} for a travel of 184 css pixels");
    }

    #[test]
    fn another_kind_is_refused_rather_than_answered() {
        let image = crop(300, 65, 12, 196);
        let mut req = request(&image, 300.0);
        req.kind = "visual".to_string();
        let reply = helper().answer(&req);
        assert_eq!(reply.dx, None);
        assert!(reply.error.expect("error").contains("slider"));
    }

    #[test]
    fn an_unknown_task_is_refused() {
        let image = crop(300, 65, 12, 196);
        let mut req = request(&image, 300.0);
        req.task = "cells".to_string();
        assert!(helper().answer(&req).error.expect("error").contains("cells"));
    }

    #[test]
    fn a_crop_with_no_notch_is_refused_not_answered_zero() {
        let flat = Gray::new(300, 65, vec![205; 300 * 65]);
        let reply = helper().answer(&request(&flat, 300.0));
        assert_eq!(reply.dx, None, "answered an axis for a crop with no notch");
        assert!(reply.error.expect("error").contains("notch"));
    }

    #[test]
    fn a_payload_that_is_not_a_png_is_refused() {
        let mut req = request(&crop(300, 65, 12, 196), 300.0);
        req.png = base64_encode(b"this is not a png");
        assert!(helper().answer(&req).error.expect("error").contains("png"));
    }

    #[test]
    fn a_colour_crop_is_reduced_to_luminance_and_still_answered() {
        let gray = crop(300, 65, 12, 196);
        let mut rgba = Vec::with_capacity(gray.pixels.len() * 4);
        for value in &gray.pixels {
            rgba.extend_from_slice(&[*value, *value, *value, 255]);
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 300, 65);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&rgba).expect("pixels");
        }
        let req: proto::Request = serde_json::from_value(serde_json::json!({
            "kind": "slider",
            "task": "axis",
            "png": base64_encode(&out),
            "width": 300.0,
            "height": 65.0,
        }))
        .expect("request");
        let dx = helper().answer(&req).dx.expect("an axis from an rgba crop");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx}");
    }
}
