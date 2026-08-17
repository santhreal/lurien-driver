//! The slider helper: one crop in, one axis out.
//!
//! The engine snapshots the widget's own browsing context, sends the crop here,
//! and drags by the answer through the trusted input path. This process sees a
//! few hundred pixels and nothing else, which is why it is a process: perception
//! does not belong in libxul, and a model that cannot see a session cannot leak
//! one.

pub mod gap;
pub mod proto;
pub mod server;

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

/// Answer one request.
///
/// The travel is reported in the caller's own coordinates: the crop may have been
/// snapshotted at a device scale, so pixels are converted back by the ratio
/// between the stated width and the decoded width. Dragging by device pixels on a
/// scaled display is the classic off-by-a-factor miss.
#[must_use]
pub fn answer(request: &proto::Request) -> proto::Reply {
    if request.kind != "slider" {
        return proto::Reply::refused(format!(
            "this helper answers the slider kind, not {}",
            request.kind
        ));
    }
    if request.task != "axis" {
        return proto::Reply::refused(format!("unknown task {}", request.task));
    }
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

    #[test]
    fn a_png_crop_round_trips_into_an_axis() {
        let image = crop(300, 65, 12, 196);
        let reply = answer(&request(&image, 300.0));
        let dx = reply.dx.expect("an axis for a crop with a notch");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx} for a travel of 184");
        assert_eq!(reply.error, None);
    }

    #[test]
    fn a_scaled_snapshot_is_reported_in_css_pixels() {
        // Snapshot taken at 2x: the crop is 600 device pixels wide but the widget
        // is 300 CSS pixels, and the drag happens in CSS pixels.
        let image = crop(600, 130, 24, 392);
        let reply = answer(&request(&image, 300.0));
        let dx = reply.dx.expect("an axis");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx} for a travel of 184 css pixels");
    }

    #[test]
    fn another_kind_is_refused_rather_than_answered() {
        let image = crop(300, 65, 12, 196);
        let mut req = request(&image, 300.0);
        req.kind = "visual".to_string();
        let reply = answer(&req);
        assert_eq!(reply.dx, None);
        assert!(reply.error.expect("error").contains("slider"));
    }

    #[test]
    fn an_unknown_task_is_refused() {
        let image = crop(300, 65, 12, 196);
        let mut req = request(&image, 300.0);
        req.task = "cells".to_string();
        assert!(answer(&req).error.expect("error").contains("cells"));
    }

    #[test]
    fn a_crop_with_no_notch_is_refused_not_answered_zero() {
        let flat = Gray::new(300, 65, vec![205; 300 * 65]);
        let reply = answer(&request(&flat, 300.0));
        assert_eq!(reply.dx, None, "answered an axis for a crop with no notch");
        assert!(reply.error.expect("error").contains("notch"));
    }

    #[test]
    fn a_payload_that_is_not_a_png_is_refused() {
        let mut req = request(&crop(300, 65, 12, 196), 300.0);
        req.png = base64_encode(b"this is not a png");
        assert!(answer(&req).error.expect("error").contains("png"));
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
        let dx = answer(&req).dx.expect("an axis from an rgba crop");
        assert!((dx - 184.0).abs() <= 2.0, "reported {dx}");
    }
}
