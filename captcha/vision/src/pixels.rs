//! Colour pixels, the little of them a classifier needs.
//!
//! The slider only ever needed luminance. A grid has to be recognised, not
//! measured, so the crop is kept in RGB and resampled to the size the model was
//! trained at. Everything here is arithmetic over one crop: no file, no network,
//! no page.

/// An RGB image, three bytes per pixel, row major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Rgb {
    /// Wrap raw RGB bytes.
    ///
    /// # Panics
    /// If `data` is not exactly `width * height * 3` bytes.
    #[must_use]
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width * height * 3, "rgb data does not fill the frame");
        Self { width, height, data }
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// One pixel, clamped to the frame so a resampler's tap can run off an edge.
    #[must_use]
    pub fn at(&self, x: isize, y: isize) -> [f32; 3] {
        let cx = x.clamp(0, self.width as isize - 1) as usize;
        let cy = y.clamp(0, self.height as isize - 1) as usize;
        let i = (cy * self.width + cx) * 3;
        [
            f32::from(self.data[i]),
            f32::from(self.data[i + 1]),
            f32::from(self.data[i + 2]),
        ]
    }

    /// The rectangle, clipped to the frame.
    ///
    /// # Errors
    /// If the rectangle lies entirely outside the frame, or has no area, which
    /// would hand the model an empty tensor instead of a cell.
    pub fn crop(&self, x: i64, y: i64, w: i64, h: i64) -> Result<Self, String> {
        let x0 = x.max(0).min(self.width as i64);
        let y0 = y.max(0).min(self.height as i64);
        let x1 = (x + w).max(0).min(self.width as i64);
        let y1 = (y + h).max(0).min(self.height as i64);
        if x1 <= x0 || y1 <= y0 {
            return Err(format!(
                "cell {x},{y} {w}x{h} does not overlap the {}x{} crop",
                self.width, self.height
            ));
        }
        let (x0, y0, cw, ch) = (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize);
        let mut data = Vec::with_capacity(cw * ch * 3);
        for row in 0..ch {
            let start = ((y0 + row) * self.width + x0) * 3;
            data.extend_from_slice(&self.data[start..start + cw * 3]);
        }
        Ok(Self::new(cw, ch, data))
    }

    /// Resample to `side` by `side` with a Catmull-Rom kernel, the bicubic the
    /// model's own preprocessing uses.
    ///
    /// A cell arrives at whatever size the widget painted it, and a nearest
    /// neighbour resize of a thin drawn shape loses the shape.
    #[must_use]
    pub fn resize_square(&self, side: usize) -> Self {
        let mut data = vec![0u8; side * side * 3];
        let sx = self.width as f32 / side as f32;
        let sy = self.height as f32 / side as f32;
        for oy in 0..side {
            // Sample at pixel centres, which is what keeps a resize from
            // shifting the image half a pixel toward the origin.
            let fy = (oy as f32 + 0.5) * sy - 0.5;
            let iy = fy.floor();
            let ty = fy - iy;
            for ox in 0..side {
                let fx = (ox as f32 + 0.5) * sx - 0.5;
                let ix = fx.floor();
                let tx = fx - ix;
                let mut acc = [0f32; 3];
                for (m, wy) in catmull_rom(ty).iter().enumerate() {
                    let mut row = [0f32; 3];
                    for (n, wx) in catmull_rom(tx).iter().enumerate() {
                        let p = self.at(ix as isize + n as isize - 1, iy as isize + m as isize - 1);
                        for c in 0..3 {
                            row[c] += p[c] * wx;
                        }
                    }
                    for c in 0..3 {
                        acc[c] += row[c] * wy;
                    }
                }
                let i = (oy * side + ox) * 3;
                for c in 0..3 {
                    data[i + c] = acc[c].round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Self::new(side, side, data)
    }
}

/// The four Catmull-Rom taps for a fractional position.
fn catmull_rom(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// Decode a PNG crop to RGB.
///
/// # Errors
/// If the bytes are not a PNG this build can read, or the frame is empty.
pub fn decode_png_rgb(bytes: &[u8]) -> Result<Rgb, String> {
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
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(format!("unsupported png bit depth {:?}", frame.bit_depth));
    }
    let channels = match frame.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        other => return Err(format!("unsupported png colour type {other:?}")),
    };
    let grey = channels <= 2;
    let source = &buffer[..frame.buffer_size()];
    let mut data = Vec::with_capacity(width * height * 3);
    for chunk in source.chunks_exact(channels) {
        if grey {
            data.extend_from_slice(&[chunk[0], chunk[0], chunk[0]]);
        } else {
            data.extend_from_slice(&chunk[..3]);
        }
    }
    if data.len() != width * height * 3 {
        return Err("png rows do not fill the frame".to_string());
    }
    Ok(Rgb::new(width, height, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, colour: [u8; 3]) -> Rgb {
        let mut data = Vec::with_capacity(width * height * 3);
        for _ in 0..width * height {
            data.extend_from_slice(&colour);
        }
        Rgb::new(width, height, data)
    }

    #[test]
    fn a_crop_keeps_the_pixels_of_its_own_rectangle() {
        // Two halves, cropped apart: a cell classifier that reads its neighbour's
        // pixels votes for the neighbour's contents.
        let mut data = Vec::new();
        for _ in 0..4 {
            for _ in 0..2 {
                data.extend_from_slice(&[255, 0, 0]);
            }
            for _ in 0..2 {
                data.extend_from_slice(&[0, 0, 255]);
            }
        }
        let image = Rgb::new(4, 4, data);
        let left = image.crop(0, 0, 2, 4).expect("left half");
        let right = image.crop(2, 0, 2, 4).expect("right half");
        assert_eq!(left, solid(2, 4, [255, 0, 0]));
        assert_eq!(right, solid(2, 4, [0, 0, 255]));
    }

    #[test]
    fn a_crop_is_clipped_to_the_frame_it_came_from() {
        let image = solid(4, 4, [10, 20, 30]);
        let clipped = image.crop(3, 3, 8, 8).expect("overlapping corner");
        assert_eq!(clipped.width(), 1);
        assert_eq!(clipped.height(), 1);
    }

    #[test]
    fn a_cell_outside_the_crop_is_refused_rather_than_emptied() {
        let image = solid(4, 4, [10, 20, 30]);
        let error = image.crop(9, 0, 2, 2).expect_err("no overlap");
        assert!(error.contains("does not overlap"), "{error}");
        let flat = image.crop(0, 0, 0, 2).expect_err("no area");
        assert!(flat.contains("does not overlap"), "{flat}");
    }

    #[test]
    fn a_resize_of_one_colour_is_that_colour() {
        // The kernel has negative lobes, so a flat field is the case that catches
        // taps that do not sum to one: ringing would show up as drift here.
        let resized = solid(37, 11, [64, 128, 192]).resize_square(224);
        assert_eq!(resized.width(), 224);
        assert_eq!(resized.height(), 224);
        for y in [0, 113, 223] {
            for x in [0, 57, 223] {
                assert_eq!(resized.at(x, y), [64.0, 128.0, 192.0], "at {x},{y}");
            }
        }
    }

    #[test]
    fn a_resize_keeps_which_side_is_which() {
        // Upscaling a two-pixel gradient: the dark end must stay on the left. A
        // transposed or mirrored resample scores every asymmetric shape wrongly
        // and is invisible in a mean-brightness check.
        let image = Rgb::new(2, 1, vec![0, 0, 0, 255, 255, 255]);
        let resized = image.resize_square(8);
        assert!(resized.at(0, 4)[0] < 64.0, "left edge {:?}", resized.at(0, 4));
        assert!(resized.at(7, 4)[0] > 192.0, "right edge {:?}", resized.at(7, 4));
    }

    #[test]
    fn a_grey_png_decodes_to_grey_rgb() {
        // The snapshot may arrive greyscale, and a decoder that only handles RGBA
        // would refuse a perfectly good crop.
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 2, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[7, 200]).expect("data");
        }
        let decoded = decode_png_rgb(&png_bytes).expect("grey png");
        assert_eq!(decoded.at(0, 0), [7.0, 7.0, 7.0]);
        assert_eq!(decoded.at(1, 0), [200.0, 200.0, 200.0]);
    }
}
