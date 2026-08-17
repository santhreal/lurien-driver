//! The classifier: one crop and one phrase in, one similarity out.
//!
//! A grid challenge asks a question in words ("select all squares with a
//! bicycle") about pictures, so the model has to compare the two. That is what a
//! contrastive image-text model does: both sides are projected into one space and
//! compared by angle.
//!
//! The weights are not in this repository and are not downloaded by this process.
//! A build points at a directory with a model in it, and a helper with no model
//! refuses every grid request by name instead of guessing cells. Perception lives
//! here, out of the browser, and this process still sees nothing but a crop.

use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::pixels::Rgb;

/// Side of the square the vision tower expects.
pub const SIDE: usize = 224;

/// Channel means and deviations the weights were trained with. Preprocessing that
/// does not match the training normalisation shifts every embedding by the same
/// wrong offset, which looks like a model that is merely bad.
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

/// Files the model directory has to hold.
const FILES: [&str; 3] = ["vision_model.onnx", "text_model.onnx", "tokenizer.json"];

/// Environment variable naming the model directory.
pub const MODEL_ENV: &str = "LURIEN_VISION_MODEL";

/// A loaded image-text model.
pub struct Clip {
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
    dir: PathBuf,
}

impl Clip {
    /// Load the model in `dir`.
    ///
    /// # Errors
    /// If a file is missing, if the runtime library cannot be loaded, or if a
    /// graph does not have the inputs this code feeds. Every message names the
    /// directory and what to do about it, because a helper that cannot classify
    /// is an operational state a caller has to fix, not a bug.
    pub fn load(dir: &Path) -> Result<Self, String> {
        for file in FILES {
            let path = dir.join(file);
            if !path.is_file() {
                return Err(format!(
                    "no {file} in {}: the grid classifier needs {} in one directory. \
                     Point {MODEL_ENV} at a CLIP export that has them, or run the helper \
                     without a model and grid requests will be refused rather than guessed",
                    dir.display(),
                    FILES.join(", ")
                ));
            }
        }
        // Two threads per tower: a grid is nine small crops on a machine that is
        // also running a browser, and a session that grabs every core makes the
        // page it is solving for slower.
        let session = |file: &str| -> Result<Session, String> {
            let path = dir.join(file);
            let fail = |e: String| {
                format!(
                    "cannot open {}: {e}. The onnxruntime library is loaded at run time; \
                     set ORT_DYLIB_PATH to a libonnxruntime.so this build can use",
                    path.display()
                )
            };
            let builder = Session::builder().map_err(|e| fail(e.to_string()))?;
            let mut builder = builder
                .with_intra_threads(2)
                .map_err(|e| fail(e.to_string()))?;
            builder
                .commit_from_file(&path)
                .map_err(|e| fail(e.to_string()))
        };
        let vision = session(FILES[0])?;
        let text = session(FILES[1])?;
        let tokenizer = Tokenizer::from_file(dir.join(FILES[2]))
            .map_err(|e| format!("cannot read {}: {e}", dir.join(FILES[2]).display()))?;
        Ok(Self {
            vision,
            text,
            tokenizer,
            dir: dir.to_path_buf(),
        })
    }

    /// The directory this model came from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Embed one crop.
    ///
    /// # Errors
    /// If the graph rejects the tensor or answers with a shape this code cannot
    /// read.
    pub fn image_embedding(&mut self, crop: &Rgb) -> Result<Vec<f32>, String> {
        let square = crop.resize_square(SIDE);
        // Channel-first, normalised: the layout the export declares, not the
        // layout a pixel buffer happens to have.
        let mut pixels = vec![0f32; 3 * SIDE * SIDE];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let rgb = square.at(x as isize, y as isize);
                for c in 0..3 {
                    pixels[c * SIDE * SIDE + y * SIDE + x] = (rgb[c] / 255.0 - MEAN[c]) / STD[c];
                }
            }
        }
        let tensor = Tensor::from_array(([1, 3, SIDE, SIDE], pixels))
            .map_err(|e| format!("cannot build the pixel tensor: {e}"))?;
        let outputs = self
            .vision
            .run(ort::inputs!["pixel_values" => tensor])
            .map_err(|e| format!("the vision tower refused the crop: {e}"))?;
        normalized(&outputs, "image_embeds")
    }

    /// Embed one phrase.
    ///
    /// Phrases are embedded one at a time on purpose. This export takes ids and
    /// no attention mask, so a padded batch feeds the model the padding as if it
    /// were words, and the phrases in a grid request differ in length.
    ///
    /// # Errors
    /// If the phrase cannot be tokenized, or the graph answers with a shape this
    /// code cannot read.
    pub fn text_embedding(&mut self, phrase: &str) -> Result<Vec<f32>, String> {
        let encoded = self
            .tokenizer
            .encode(phrase, true)
            .map_err(|e| format!("cannot tokenize {phrase:?}: {e}"))?;
        let ids: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
        if ids.is_empty() {
            return Err(format!("{phrase:?} tokenizes to nothing"));
        }
        let len = ids.len();
        let tensor = Tensor::from_array(([1, len], ids))
            .map_err(|e| format!("cannot build the token tensor: {e}"))?;
        let outputs = self
            .text
            .run(ort::inputs!["input_ids" => tensor])
            .map_err(|e| format!("the text tower refused {phrase:?}: {e}"))?;
        normalized(&outputs, "text_embeds")
    }
}

/// Read one embedding out of a session's outputs, scaled to unit length so a
/// comparison is an angle and not a magnitude.
fn normalized(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> Result<Vec<f32>, String> {
    let value = outputs
        .get(name)
        .ok_or_else(|| format!("the model has no {name} output"))?;
    let (_, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    let norm = data.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(format!("{name} has no length"));
    }
    Ok(data.iter().map(|v| v / norm).collect())
}

/// Angle between two unit embeddings, as a similarity in `-1..=1`.
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
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
        let Err(error) = Clip::load(Path::new("/nonexistent/model/dir")) else {
            panic!("a directory with no model loaded a model");
        };
        for file in FILES {
            assert!(error.contains(file), "{file} not named: {error}");
        }
        assert!(error.contains(MODEL_ENV), "no variable named: {error}");
    }

    #[test]
    fn a_similarity_is_an_angle_and_not_a_length() {
        let a = [1.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [0.0, 1.0, 0.0];
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-6);
        assert!(cosine(&a, &c).abs() < 1e-6);
    }
}
