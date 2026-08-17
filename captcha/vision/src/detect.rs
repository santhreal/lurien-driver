//! The detector: one crop and the words for an object in, boxes out.
//!
//! A grid challenge asks for a thing ("select all squares with a bicycle") and
//! pays out all or nothing, so the question is not what a tile is a picture of, it
//! is whether the thing is anywhere in that tile. Those are different questions.
//! A whole-tile classifier answers the first: a reCAPTCHA tile holding a car at
//! the far end of a street is mostly street, and a caption model says street.
//! Measured on a live 3x3 grid whose answer was tiles 2, 4 and 7, whole-tile
//! captioning picked 2, 4 and 6, and its score for `car` sat within 0.01 of the
//! best wrong class on every tile.
//!
//! An open-vocabulary detector answers the second question directly: it proposes
//! boxes and scores each box against the phrase, so a small object in a busy tile
//! is a confident box rather than a diluted caption. On the same crop it returned
//! six boxes, all of them inside tiles 2, 4 and 7.
//!
//! The weights are not in this repository and are not downloaded by this process.
//! A helper with no model refuses every grid request by name instead of guessing
//! cells. Perception lives here, out of the browser, and this process still sees
//! nothing but a crop.

use std::path::{Path, PathBuf};

use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::pixels::Rgb;

/// Side of the square the image branch expects.
pub const SIDE: usize = 768;

/// Channel means and deviations the weights were trained with. Preprocessing that
/// does not match the training normalisation shifts every score by the same wrong
/// offset, which looks like a model that is merely bad.
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

/// Tokens every query is padded to, with the attention mask that hides the
/// padding. The graph takes one batch of equal-length queries, and the phrases a
/// widget asks for are short.
const QUERY_TOKENS: usize = 16;

/// Files the model directory has to hold.
const FILES: [&str; 2] = ["model.onnx", "tokenizer.json"];

/// Environment variable naming the model directory.
pub const MODEL_ENV: &str = "LURIEN_VISION_MODEL";

/// One object the detector found, in the crop's own pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    /// The detector's own confidence, in `0..=1`.
    pub score: f32,
    /// Which query matched, as an index into the queries that were asked.
    pub query: usize,
    /// Box centre, x.
    pub cx: f64,
    /// Box centre, y.
    pub cy: f64,
    /// Box width.
    pub w: f64,
    /// Box height.
    pub h: f64,
}

/// A loaded detector.
pub struct Detector {
    session: Session,
    tokenizer: Tokenizer,
    dir: PathBuf,
}

impl Detector {
    /// Load the model in `dir`.
    ///
    /// # Errors
    /// If a file is missing, if the runtime library cannot be loaded, or if the
    /// graph does not have the inputs this code feeds. Every message names the
    /// directory and what to do about it, because a helper that cannot see is an
    /// operational state a caller has to fix, not a bug.
    pub fn load(dir: &Path) -> Result<Self, String> {
        for file in FILES {
            let path = dir.join(file);
            if !path.is_file() {
                return Err(format!(
                    "no {file} in {}: the grid detector needs {} in one directory. \
                     Point {MODEL_ENV} at an open-vocabulary detector export that has \
                     them, or run the helper without a model and grid requests will be \
                     refused rather than guessed",
                    dir.display(),
                    FILES.join(", ")
                ));
            }
        }
        let path = dir.join(FILES[0]);
        let fail = |e: String| {
            format!(
                "cannot open {}: {e}. The onnxruntime library is loaded at run time; \
                 set ORT_DYLIB_PATH to a libonnxruntime.so this build can use",
                path.display()
            )
        };
        // Two threads: a grid is one pass on a machine that is also running a
        // browser, and a session that grabs every core makes the page it is
        // solving for slower.
        let builder = Session::builder().map_err(|e| fail(e.to_string()))?;
        let mut builder = builder
            .with_intra_threads(2)
            .map_err(|e| fail(e.to_string()))?;
        let session = builder.commit_from_file(&path).map_err(|e| fail(e.to_string()))?;
        let tokenizer = Tokenizer::from_file(dir.join(FILES[1]))
            .map_err(|e| format!("cannot read {}: {e}", dir.join(FILES[1]).display()))?;
        Ok(Self {
            session,
            tokenizer,
            dir: dir.to_path_buf(),
        })
    }

    /// The directory this model came from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Objects in `crop` matching any of `queries`, scoring at or above `floor`,
    /// strongest first.
    ///
    /// Boxes come back in the crop's own pixels: the whole grid is looked at once,
    /// so a caller with cell rectangles can say which cell a box landed in without
    /// cropping anything itself.
    ///
    /// # Errors
    /// If a query cannot be tokenized, if the graph rejects the tensors, or if it
    /// answers with a shape this code cannot read.
    pub fn detect(
        &mut self,
        crop: &Rgb,
        queries: &[String],
        floor: f32,
    ) -> Result<Vec<Detection>, String> {
        if queries.is_empty() {
            return Err("a detection needs at least one query".to_string());
        }
        let (ids, mask) = self.encode(queries)?;
        let pixels = normalized_pixels(crop);
        let ids = Tensor::from_array(([queries.len(), QUERY_TOKENS], ids))
            .map_err(|e| format!("cannot build the token tensor: {e}"))?;
        let mask = Tensor::from_array(([queries.len(), QUERY_TOKENS], mask))
            .map_err(|e| format!("cannot build the mask tensor: {e}"))?;
        let pixels = Tensor::from_array(([1, 3, SIDE, SIDE], pixels))
            .map_err(|e| format!("cannot build the pixel tensor: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => ids,
                "attention_mask" => mask,
                "pixel_values" => pixels,
            ])
            .map_err(|e| format!("the detector refused the crop: {e}"))?;
        let logits = tensor_data(&outputs, "logits")?;
        let boxes = tensor_data(&outputs, "pred_boxes")?;
        read_detections(
            &logits,
            &boxes,
            queries.len(),
            crop.width() as f64,
            crop.height() as f64,
            floor,
        )
    }

    /// Every query as a padded row of token ids, with the mask that hides the
    /// padding from the text branch.
    fn encode(&self, queries: &[String]) -> Result<(Vec<i64>, Vec<i64>), String> {
        let mut ids = vec![0i64; queries.len() * QUERY_TOKENS];
        let mut mask = vec![0i64; queries.len() * QUERY_TOKENS];
        for (row, query) in queries.iter().enumerate() {
            let encoded = self
                .tokenizer
                .encode(query.as_str(), true)
                .map_err(|e| format!("cannot tokenize {query:?}: {e}"))?;
            let tokens = encoded.get_ids();
            if tokens.is_empty() {
                return Err(format!("{query:?} tokenizes to nothing"));
            }
            // A phrase longer than the row is truncated rather than refused: the
            // words a widget puts first are the object it is asking about.
            for (column, &token) in tokens.iter().take(QUERY_TOKENS).enumerate() {
                ids[row * QUERY_TOKENS + column] = i64::from(token);
                mask[row * QUERY_TOKENS + column] = 1;
            }
        }
        Ok((ids, mask))
    }
}

/// The crop as the tensor the graph declares: square, channel-first, normalised.
fn normalized_pixels(crop: &Rgb) -> Vec<f32> {
    let square = crop.resize_square(SIDE);
    let mut pixels = vec![0f32; 3 * SIDE * SIDE];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let rgb = square.at(x as isize, y as isize);
            for c in 0..3 {
                pixels[c * SIDE * SIDE + y * SIDE + x] = (rgb[c] / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    pixels
}

/// One float tensor out of a session's outputs.
fn tensor_data(outputs: &SessionOutputs<'_>, name: &str) -> Result<Vec<f32>, String> {
    let value = outputs
        .get(name)
        .ok_or_else(|| format!("the model has no {name} output"))?;
    let (_, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    Ok(data.to_vec())
}

/// Turn the graph's two tensors into boxes in the crop's pixels.
///
/// The shapes are derived from the data rather than trusted: `logits` is one score
/// per patch per query and `pred_boxes` four numbers per patch, so a graph whose
/// patch count changed with a different export is read correctly, and one whose
/// tensors do not agree is an error instead of a box read from the wrong offset.
fn read_detections(
    logits: &[f32],
    boxes: &[f32],
    queries: usize,
    width: f64,
    height: f64,
    floor: f32,
) -> Result<Vec<Detection>, String> {
    if queries == 0 {
        return Err("a detection needs at least one query".to_string());
    }
    if logits.is_empty() || logits.len() % queries != 0 {
        return Err(format!(
            "the model answered {} scores for {queries} queries, which is not a score per patch",
            logits.len()
        ));
    }
    let patches = logits.len() / queries;
    if boxes.len() != patches * 4 {
        return Err(format!(
            "the model answered {} box numbers for {patches} patches, not {}",
            boxes.len(),
            patches * 4
        ));
    }
    let mut found = Vec::new();
    for patch in 0..patches {
        for query in 0..queries {
            let score = sigmoid(logits[patch * queries + query]);
            if score < floor {
                continue;
            }
            // Centre, width and height, as fractions of the square the model saw.
            // The crop was squashed to that square, so each axis scales back on
            // its own.
            let box_at = patch * 4;
            found.push(Detection {
                score,
                query,
                cx: f64::from(boxes[box_at]) * width,
                cy: f64::from(boxes[box_at + 1]) * height,
                w: f64::from(boxes[box_at + 2]) * width,
                h: f64::from(boxes[box_at + 3]) * height,
            });
        }
    }
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(found)
}

/// A logit as a probability. The detector scores every box against every query on
/// its own, so this is not a share of anything: two boxes can both be certain.
#[must_use]
pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The model directory a caller named, if any.
#[must_use]
pub fn model_dir_from_env() -> Option<PathBuf> {
    match std::env::var(MODEL_ENV) {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_names_the_files_and_the_variable() {
        let Err(error) = Detector::load(Path::new("/nonexistent/model/dir")) else {
            panic!("a directory with no model loaded a model");
        };
        for file in FILES {
            assert!(error.contains(file), "{file} not named: {error}");
        }
        assert!(error.contains(MODEL_ENV), "no variable named: {error}");
    }

    #[test]
    fn a_score_is_per_box_and_not_a_share() {
        // Independent scores: the detector can be sure about two boxes at once,
        // which is what a grid with three matching tiles looks like.
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(-10.0) < 1e-4);
        assert!(sigmoid(10.0) > 0.999);
    }

    /// Two patches, two queries, in the row-major order the graph declares.
    fn tensors() -> (Vec<f32>, Vec<f32>) {
        let logits = vec![
            // patch 0: strong for query 1 only
            -6.0, 2.0,
            // patch 1: strong for query 0 only
            1.0, -8.0,
        ];
        let boxes = vec![
            // patch 0 centred a quarter in, a tenth of the frame wide
            0.25, 0.25, 0.1, 0.1,
            // patch 1 centred three quarters in
            0.75, 0.5, 0.2, 0.2,
        ];
        (logits, boxes)
    }

    #[test]
    fn a_box_comes_back_in_the_crops_own_pixels() {
        let (logits, boxes) = tensors();
        let found = read_detections(&logits, &boxes, 2, 400.0, 200.0, 0.5).expect("two boxes");
        assert_eq!(found.len(), 2, "{found:?}");
        // Strongest first, and each axis scaled by its own side: the crop was
        // squashed to a square, so a box is not square in the crop.
        assert_eq!(found[0].query, 1);
        assert!((found[0].cx - 100.0).abs() < 1e-6, "{:?}", found[0]);
        assert!((found[0].cy - 50.0).abs() < 1e-6, "{:?}", found[0]);
        assert!((found[0].w - 40.0).abs() < 1e-6, "{:?}", found[0]);
        assert!((found[0].h - 20.0).abs() < 1e-6, "{:?}", found[0]);
        assert_eq!(found[1].query, 0);
        assert!((found[1].cx - 300.0).abs() < 1e-6, "{:?}", found[1]);
    }

    #[test]
    fn a_score_under_the_floor_is_not_a_box() {
        let (logits, boxes) = tensors();
        let found = read_detections(&logits, &boxes, 2, 100.0, 100.0, 0.9).expect("read");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn tensors_that_do_not_agree_are_an_error_and_not_a_box() {
        // Reading four numbers per patch out of a shorter tensor would answer with
        // a box built from whatever floats followed it, which is a confident click
        // on nothing.
        let (logits, mut boxes) = tensors();
        boxes.truncate(5);
        let error = read_detections(&logits, &boxes, 2, 100.0, 100.0, 0.5).expect_err("a refusal");
        assert!(error.contains("box numbers"), "{error}");
        let error = read_detections(&logits, &boxes, 3, 100.0, 100.0, 0.5).expect_err("a refusal");
        assert!(error.contains("score per patch"), "{error}");
        let error = read_detections(&[], &[], 1, 100.0, 100.0, 0.5).expect_err("a refusal");
        assert!(error.contains("score per patch"), "{error}");
    }
}
