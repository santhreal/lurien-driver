//! Where is the notch, and how far is the piece from it.
//!
//! A slider challenge is one image: a puzzle piece parked at the left and a notch
//! cut out of the background somewhere to the right. The answer is one number,
//! the horizontal distance between them, and it is recoverable from the image
//! with arithmetic. No model is involved, which is why this runs in a helper
//! process of a few hundred lines instead of an inference runtime.
//!
//! Both the piece and the notch have hard vertical edges, and a hard vertical
//! edge is exactly what a horizontal gradient answers to. Summing that gradient
//! down each column turns the image into a one-dimensional signal with a spike at
//! every vertical edge, and the two spikes that matter are the leftmost one (the
//! piece) and the strongest remaining one (the notch).

/// A decoded crop, one byte of luminance per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Gray {
    /// # Panics
    /// If `pixels` is not exactly `width * height` long.
    #[must_use]
    pub fn new(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        assert_eq!(pixels.len(), width * height, "pixel count does not match the frame");
        Self { width, height, pixels }
    }

    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * self.width + x]
    }
}

/// What the helper found, in the coordinates of the crop it was given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    /// Column of the piece's leading edge.
    pub piece_x: usize,
    /// Column of the notch's leading edge.
    pub gap_x: usize,
    /// Pixels the piece has to travel.
    pub dx: usize,
    /// Notch spike height over the median column, in units of the median. A weak
    /// spike means the image has no notch and the answer would be a guess.
    pub confidence: f32,
}

/// Vertical-edge energy per column: the absolute horizontal gradient summed down
/// the column, normalized by height so the value is per pixel and comparable
/// between crops of different sizes.
#[must_use]
pub fn column_energy(image: &Gray) -> Vec<f32> {
    let mut energy = vec![0.0; image.width];
    if image.width < 3 || image.height == 0 {
        return energy;
    }
    for x in 1..image.width - 1 {
        let mut sum = 0.0f32;
        for y in 0..image.height {
            let left = f32::from(image.at(x - 1, y));
            let right = f32::from(image.at(x + 1, y));
            sum += (right - left).abs();
        }
        energy[x] = sum / image.height as f32;
    }
    energy
}

/// Median of a column-energy signal, used as the noise floor.
fn median(values: &[f32]) -> f32 {
    let mut sorted: Vec<f32> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    sorted[sorted.len() / 2]
}

/// Peaks in a signal, strongest first, with non-maximum suppression so one edge
/// two pixels wide is one peak rather than three.
fn peaks(energy: &[f32], separation: usize) -> Vec<(usize, f32)> {
    let mut candidates: Vec<(usize, f32)> = energy
        .iter()
        .copied()
        .enumerate()
        .filter(|(x, value)| {
            *value > 0.0
                && energy.get(x.wrapping_sub(1)).copied().unwrap_or(0.0) <= *value
                && energy.get(x + 1).copied().unwrap_or(0.0) <= *value
        })
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("finite"));
    let mut kept: Vec<(usize, f32)> = Vec::new();
    for (x, value) in candidates {
        if kept
            .iter()
            .all(|(taken, _)| x.abs_diff(*taken) >= separation)
        {
            kept.push((x, value));
        }
    }
    kept
}

/// Minimum spike-over-floor ratio before an answer is offered at all.
const MIN_CONFIDENCE: f32 = 2.0;

/// Smallest travel worth reporting. A notch found on top of the piece is a
/// misread, not a challenge that was already solved.
const MIN_TRAVEL: usize = 8;

/// Find the piece and the notch.
///
/// Both are shapes, not lines: each contributes two spikes, a leading and a
/// trailing edge. Treating every spike as a candidate is the mistake that makes a
/// public solver drag by the width of its own puzzle piece, and treating the
/// leftmost spike as the piece is the mistake that makes it drag by the widget's
/// border inset.
///
/// The invariant every slider puzzle holds is that the piece and the cut-out are
/// the same shape, so they are the same width. This searches for two disjoint
/// edge pairs of equal width and scores each candidate by its weakest edge, which
/// is what separates a real pair of shapes from a border edge that happens to sit
/// one shape width from something else. Where no such pair exists the leftmost
/// shape is taken as the piece and the strongest edge past it as the notch, which
/// is all a crop with one occluded edge supports.
///
/// Returns `None` when the crop carries no vertical edge that stands out from its
/// own noise floor: a helper that answers zero for a picture of a wall teaches the
/// engine to drag for nothing.
#[must_use]
pub fn find(image: &Gray) -> Option<Gap> {
    let energy = column_energy(image);
    let floor = median(&energy).max(0.05);
    let mut strong: Vec<(usize, f32)> = peaks(&energy, (image.width / 40).max(3))
        .into_iter()
        .filter(|(_, value)| *value / floor >= MIN_CONFIDENCE)
        .collect();
    if strong.len() < 2 {
        return None;
    }
    strong.sort_by_key(|(x, _)| *x);
    // A shape wider than a third of the crop is not a puzzle piece, and one
    // narrower than a few pixels is a line.
    let widest_shape = (image.width / 3).max(12);
    if let Some(found) = matched_pair(&strong, widest_shape, floor) {
        return Some(found);
    }
    fallback(&strong, widest_shape, floor)
}

/// Narrowest shape that can be a puzzle piece. Two edges closer than this are one
/// edge that survived suppression twice, or a line in the artwork.
const MIN_SHAPE: usize = 6;

/// How far two shape widths may differ and still be the same shape, in pixels.
/// A cut-out is antialiased against its background, so its measured edges sit a
/// pixel or two inside the piece's.
const WIDTH_SLACK: usize = 3;

/// The piece and the notch as two equal-width edge pairs.
///
/// Scored by the weakest of the four edges: a candidate that needs a spike barely
/// above the noise floor to exist is a coincidence, and the true pair has four
/// real edges. Near-equal scores go to the leftmost piece, because the piece is
/// parked at the left of every slider puzzle.
fn matched_pair(strong: &[(usize, f32)], widest_shape: usize, floor: f32) -> Option<Gap> {
    let mut best: Option<(f32, usize, usize)> = None;
    for (i, (piece_x, e0)) in strong.iter().enumerate() {
        for (piece_end, e1) in strong.iter().skip(i + 1) {
            let width = piece_end - piece_x;
            if width < MIN_SHAPE || width > widest_shape {
                continue;
            }
            for (k, (gap_x, e2)) in strong.iter().enumerate().skip(i + 2) {
                if *gap_x <= *piece_end || gap_x - piece_x < MIN_TRAVEL {
                    continue;
                }
                for (gap_end, e3) in strong.iter().skip(k + 1) {
                    if (gap_end - gap_x).abs_diff(width) > WIDTH_SLACK {
                        continue;
                    }
                    let score = e0.min(*e1).min(*e2).min(*e3);
                    let better = match best {
                        None => true,
                        // A tie goes to the leftmost piece, then to the shorter
                        // travel: a further notch needs a stronger case.
                        Some((top, best_piece, best_gap)) => {
                            score > top * 1.05
                                || (score > top * 0.95
                                    && (*piece_x, *gap_x) < (best_piece, best_gap))
                        }
                    };
                    if better {
                        best = Some((score, *piece_x, *gap_x));
                    }
                }
            }
        }
    }
    let (score, piece_x, gap_x) = best?;
    Some(Gap {
        piece_x,
        gap_x,
        dx: gap_x - piece_x,
        confidence: score / floor,
    })
}

/// One shape and one edge: the crop shows a piece whose cut-out has only one
/// readable side, so the notch is the strongest edge past the piece.
fn fallback(strong: &[(usize, f32)], widest_shape: usize, floor: f32) -> Option<Gap> {
    let (piece_x, _) = *strong.first()?;
    let piece_width = strong
        .iter()
        .skip(1)
        .find(|(x, _)| *x - piece_x <= widest_shape)
        .map_or(0, |(x, _)| *x - piece_x);
    let search_from = piece_x + piece_width.max(MIN_TRAVEL) + 2;
    let (gap_x, spike) = strong
        .iter()
        .copied()
        .filter(|(x, _)| *x >= search_from)
        .max_by(|a, b| a.1.partial_cmp(&b.1).expect("finite"))?;
    Some(Gap {
        piece_x,
        gap_x,
        dx: gap_x - piece_x,
        confidence: spike / floor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slider crop: light background, a dark piece parked at `piece_x`, a dark
    /// notch at `gap_x`, both the same width.
    fn crop(width: usize, height: usize, piece_x: usize, gap_x: usize, piece_w: usize) -> Gray {
        let mut pixels = vec![220u8; width * height];
        for y in height / 4..height * 3 / 4 {
            for x in piece_x..(piece_x + piece_w).min(width) {
                pixels[y * width + x] = 40;
            }
            for x in gap_x..(gap_x + piece_w).min(width) {
                pixels[y * width + x] = 60;
            }
        }
        Gray::new(width, height, pixels)
    }

    #[test]
    fn the_travel_is_the_distance_between_the_piece_and_the_notch() {
        for gap_x in [80usize, 120, 168, 210, 244] {
            let image = crop(300, 65, 10, gap_x, 30);
            let found = find(&image).expect("a crop with a piece and a notch has an answer");
            let want = gap_x - 10;
            assert!(
                found.dx.abs_diff(want) <= 2,
                "gap at {gap_x}: reported {} for a travel of {want}",
                found.dx
            );
        }
    }

    /// The crop is the widget's own box, so a bordered widget puts a full-height
    /// edge at both ends of it. Those edges are the strongest in the image. Read as
    /// the piece, the left one makes every answer short by the piece's own inset,
    /// and the drag lands nowhere near the notch.
    #[test]
    fn the_crop_border_is_not_the_puzzle_piece() {
        for inset in [1usize, 2, 3] {
            for gap_x in [90usize, 150, 232] {
                let mut image = crop(300, 65, 10, gap_x, 34);
                for y in 0..image.height {
                    for x in 0..inset {
                        let w = image.width;
                        image.pixels[y * w + x] = 30;
                        image.pixels[y * w + (w - 1 - x)] = 30;
                    }
                }
                let found = find(&image).expect("a bordered crop still has an answer");
                let want = gap_x - 10;
                assert!(
                    found.dx.abs_diff(want) <= 2,
                    "a {inset}px border with the notch at {gap_x} reported {} for a travel of {want}",
                    found.dx
                );
            }
        }
    }

    #[test]
    fn a_crop_with_no_notch_has_no_answer() {
        // Flat background: no vertical edge stands out, so there is nothing to
        // drag towards and the helper must say so instead of answering zero.
        let flat = Gray::new(300, 65, vec![200; 300 * 65]);
        assert_eq!(find(&flat), None);
    }

    #[test]
    fn gradient_noise_is_not_a_notch() {
        // A smooth horizontal ramp has a constant gradient, so every column looks
        // the same and no peak clears the floor.
        let (w, h) = (300usize, 65usize);
        let mut pixels = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                pixels[y * w + x] = u8::try_from(x * 255 / w).expect("in range");
            }
        }
        assert_eq!(find(&Gray::new(w, h, pixels)), None);
    }

    #[test]
    fn a_notch_on_top_of_the_piece_is_not_an_answer() {
        // Two edges four pixels apart are the two sides of one shape, not a piece
        // and a target: reporting a four-pixel drag would be a misread.
        let image = crop(300, 65, 10, 14, 4);
        let found = find(&image);
        assert!(
            found.is_none() || found.expect("checked").dx >= MIN_TRAVEL,
            "reported a travel shorter than one shape: {found:?}"
        );
    }

    #[test]
    fn confidence_falls_as_the_notch_fades_into_the_background() {
        let sharp = find(&crop(300, 65, 10, 180, 30)).expect("sharp notch");
        let mut faint_pixels = vec![220u8; 300 * 65];
        for y in 16..48 {
            for x in 10..40 {
                faint_pixels[y * 300 + x] = 40;
            }
            for x in 180..210 {
                faint_pixels[y * 300 + x] = 214;
            }
        }
        let faint = find(&Gray::new(300, 65, faint_pixels));
        match faint {
            None => {}
            Some(found) => assert!(
                found.confidence < sharp.confidence,
                "a faint notch reported {} against {} for a sharp one",
                found.confidence,
                sharp.confidence
            ),
        }
    }

    #[test]
    fn column_energy_spikes_only_at_a_vertical_edge() {
        let image = crop(60, 20, 5, 40, 6);
        let energy = column_energy(&image);
        let edge = energy[40].max(energy[41]);
        let flat = energy[25];
        assert!(edge > flat * 5.0, "edge {edge} is not distinct from flat {flat}");
    }
}
