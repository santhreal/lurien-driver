//! The listener: one clip in, the characters a widget asked for out.
//!
//! An audio challenge is a short recording of an answer read aloud, and a general
//! speech model transcribes it as prose. That is the wrong output twice over: it
//! invents punctuation and words the answer cannot contain, and on a noisy clip it
//! will happily produce a fluent sentence about something else entirely. So two
//! things are done differently here.
//!
//! The binding declares the alphabet its answer is drawn from, and the decode is
//! masked to the tokens that alphabet can spell. Nothing else can be emitted, so a
//! clip of digits cannot come back as a sentence, and every step's probability is a
//! share among the answers that were actually possible.
//!
//! And the clip is read three times, at its own speed and a tenth either side of
//! it. Speaking rate is the axis a digit-by-digit recording is most fragile on: the
//! same model drops a repeated digit at one rate and hears both at another. Three
//! readings that agree are one number a caller can compare against a floor; three
//! that disagree are a refusal, which is what a reload is for.
//!
//! The weights are not in this repository and are not downloaded by this process.
//! A helper with no model refuses every audio request by name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::sound::{self, Frontend};

/// Files the model directory has to hold.
const FILES: [&str; 3] = ["encoder_model.onnx", "decoder_model.onnx", "tokenizer.json"];

/// Environment variable naming the model directory.
pub const MODEL_ENV: &str = "LURIEN_AUDIO_MODEL";

/// Speeds the clip is read at, as multiples of its own length.
///
/// One is the recording as served. The other two are a tenth slower and a tenth
/// faster, which moves the pitch with the rate, so the three readings fail
/// independently instead of making the same mistake three times.
const SPEEDS: [f64; 3] = [1.0, 1.1, 0.9];

/// Silence put before and after the clip, in seconds.
///
/// A vendor's clip starts on the first syllable, and a decode of speech that begins
/// at the very first frame loses it: measured on spoken digit codes, the first digit
/// came back missing on most clips with no lead-in and on none with half a second of
/// it. The model was trained on thirty second windows of continuous audio, where a
/// word is never the first sample.
const LEAD_S: f64 = 0.5;
const TAIL_S: f64 = 0.25;

/// Level of the room tone mixed under a clip, as a share of full scale.
const ROOM_TONE: f32 = 0.002;

/// Longest answer that will be decoded, in tokens. An audio challenge answer is a
/// handful of characters; a model looping on noise is what the cap is for.
const MAX_TOKENS: usize = 24;

/// Consecutive repeats of one token that mean the decode has stopped listening.
const REPEAT_LIMIT: usize = 4;

/// Longest silence left inside a clip, in seconds. A vendor's pauses between
/// spoken characters are longer than this, and a model reads a long pause as the
/// end of the recording.
const MAX_GAP_S: f64 = 0.15;

/// How much a reading is trusted by how many of the three readings agreed with it.
///
/// Measured over sixty synthesized challenges: a unanimous reading was exact far
/// more often than a two-of-three one, and a lone reading was worth little. Folding
/// agreement into the confidence lets a caller carry one floor instead of two
/// numbers and a rule.
const AGREEMENT: [f32; 4] = [0.0, 0.7, 0.9, 1.0];

/// What the model heard, and how sure it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// The answer, in the binding's own alphabet, ready to be typed.
    pub text: String,
    /// What the model actually emitted, before the alphabet was applied. Kept for
    /// evidence: a transcript that was rejected is easier to read than a refusal.
    pub raw: String,
    /// Mean probability of the tokens that were chosen, scaled by how many of the
    /// three readings agreed.
    pub confidence: f32,
    /// How many of the readings produced this answer.
    pub agreement: usize,
    /// All three readings, in speed order.
    pub heard: Vec<String>,
}

/// A loaded speech model.
pub struct Listener {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    front: Frontend,
    dir: PathBuf,
    start: i64,
    end: i64,
    /// The token that tells the model not to emit timestamps. A transcript with
    /// them decodes as `<|0.00|>` inside the answer.
    plain: i64,
}

impl Listener {
    /// Load the model in `dir`.
    ///
    /// # Errors
    /// If a file is missing, if the runtime library cannot be loaded, or if the
    /// tokenizer does not carry the control tokens a transcript starts and ends
    /// with. Every message names the directory and what to do about it: a helper
    /// that cannot hear is an operational state a caller has to fix.
    pub fn load(dir: &Path) -> Result<Self, String> {
        for file in FILES {
            let path = dir.join(file);
            if !path.is_file() {
                return Err(format!(
                    "no {file} in {}: the audio transcriber needs {} in one directory. \
                     Point {MODEL_ENV} at a speech recognition export that has them, or run \
                     the helper without one and audio requests will be refused rather than \
                     guessed",
                    dir.display(),
                    FILES.join(", ")
                ));
            }
        }
        let encoder = open(&dir.join(FILES[0]))?;
        let decoder = open(&dir.join(FILES[1]))?;
        let tokenizer = Tokenizer::from_file(dir.join(FILES[2]))
            .map_err(|e| format!("cannot read {}: {e}", dir.join(FILES[2]).display()))?;
        let token = |name: &str| -> Result<i64, String> {
            tokenizer
                .token_to_id(name)
                .map(i64::from)
                .ok_or_else(|| format!("the tokenizer in {} has no {name}", dir.display()))
        };
        let start = token("<|startoftranscript|>")?;
        let end = token("<|endoftext|>")?;
        let plain = token("<|notimestamps|>")?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            front: Frontend::new(),
            dir: dir.to_path_buf(),
            start,
            end,
            plain,
        })
    }

    /// The directory this model came from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Transcribe `samples`, which are mono at the model's rate.
    ///
    /// `alphabet` is the set of characters the answer may be spelled with, from the
    /// binding. An empty alphabet is an unmasked decode, which is what a widget
    /// asking for a word rather than a code needs.
    ///
    /// # Errors
    /// If the alphabet spells nothing this model can emit, or if the graph rejects
    /// the tensors.
    pub fn hear(&mut self, samples: &[f32], alphabet: &str) -> Result<Transcript, String> {
        self.hear_with(samples, alphabet, true, true, true)
    }

    /// Transcribe with individual front-end decisions toggled, for mutation tests.
    ///
    /// `tighten` shortens long pauses, `frame` adds lead and tail silence with
    /// room tone, and `mask` restricts the decode to one token per character of
    /// the alphabet. Each is on by default; turning one off is what proves it was
    /// load-bearing.
    ///
    /// # Errors
    /// If the alphabet spells nothing this model can emit, or if the graph rejects
    /// the tensors.
    pub fn hear_with(
        &mut self,
        samples: &[f32],
        alphabet: &str,
        tighten: bool,
        frame: bool,
        mask: bool,
    ) -> Result<Transcript, String> {
        if samples.is_empty() {
            return Err("the clip holds no samples, so there is nothing to transcribe".to_string());
        }
        let allowed = if mask {
            self.allowed(alphabet)?
        } else {
            // An unmasked decode still spells through the alphabet, so the text is
            // in the binding's set, but the model's mass is split over every
            // spelling it knows rather than one per character.
            None
        };
        let fixed = if tighten {
            sound::tighten(samples, MAX_GAP_S)
        } else {
            samples.to_vec()
        };
        let mut readings: Vec<(String, String, f32)> = Vec::with_capacity(SPEEDS.len());
        for speed in SPEEDS {
            let audio = if (speed - 1.0).abs() < f64::EPSILON {
                fixed.clone()
            } else {
                sound::stretch(&fixed, speed)
            };
            let normalized = sound::normalize(&audio);
            let input = if frame {
                framed(&normalized)
            } else {
                normalized
            };
            let mel = self.front.log_mel(&input);
            let hidden = self.encode(&mel)?;
            let (raw, mean) = self.decode(&hidden, allowed.as_deref())?;
            let text = spell(&raw, alphabet);
            readings.push((text, raw, mean));
        }
        Ok(vote(readings))
    }

    /// The token ids an alphabet can spell, or `None` for an unmasked decode.
    ///
    /// One character per token, with or without the word boundary marker in front
    /// of it: a digit alphabet admits `7` and ` 7` and not `42`. The vocabulary
    /// holds multi-character tokens for the same answer, and allowing them splits
    /// the model's mass over several spellings of one code, which lowers every
    /// step's share and changes what greedy decoding follows. Measured: the same
    /// clips that were read exactly with one character per token came back missing
    /// digits when `42` was a legal token, and at half the confidence.
    ///
    /// The cost is that this masks an answer that is a word into being spelled
    /// letter by letter, which is why a binding whose answer is a word declares no
    /// alphabet at all.
    fn allowed(&self, alphabet: &str) -> Result<Option<Vec<i64>>, String> {
        if alphabet.is_empty() {
            return Ok(None);
        }
        let mut ids = Vec::new();
        for character in alphabet.chars().collect::<BTreeSet<char>>() {
            for token in [character.to_string(), format!("\u{0120}{character}")] {
                if let Some(id) = self.tokenizer.token_to_id(&token) {
                    ids.push(i64::from(id));
                }
            }
        }
        if ids.is_empty() {
            return Err(format!(
                "no token in this model's vocabulary spells a single character of the alphabet \
                 {alphabet:?}, so the binding's alphabet and the model disagree"
            ));
        }
        ids.sort_unstable();
        Ok(Some(ids))
    }

    /// The encoder's states for one spectrogram, kept as data so the decoder can be
    /// fed them again on every step.
    fn encode(&mut self, mel: &[f32]) -> Result<Hidden, String> {
        let input = Tensor::from_array((
            [1, sound::MELS, sound::FRAMES],
            mel.to_vec(),
        ))
        .map_err(|e| format!("cannot build the spectrogram tensor: {e}"))?;
        let outputs = self
            .encoder
            .run(ort::inputs!["input_features" => input])
            .map_err(|e| format!("the transcriber refused the clip: {e}"))?;
        let (shape, data) = first_tensor(&outputs)?;
        if shape.len() != 3 || shape[0] != 1 {
            return Err(format!(
                "the encoder answered a shape this code cannot read: {shape:?}"
            ));
        }
        Ok(Hidden {
            frames: shape[1],
            width: shape[2],
            data,
        })
    }

    /// Greedy decode, masked to `allowed`, returning what was emitted and the mean
    /// probability of the tokens that were chosen.
    fn decode(&mut self, hidden: &Hidden, allowed: Option<&[i64]>) -> Result<(String, f32), String> {
        let mut ids = vec![self.start, self.plain];
        let mut emitted: Vec<u32> = Vec::new();
        let mut probabilities: Vec<f32> = Vec::new();
        let mut repeats = 0usize;
        for _ in 0..MAX_TOKENS {
            let logits = self.step(&ids, hidden)?;
            let (token, probability) = pick(&logits, allowed, self.end);
            if token == self.end {
                break;
            }
            if emitted.last().copied() == Some(token as u32) {
                repeats += 1;
                if repeats >= REPEAT_LIMIT {
                    // A model repeating one token has stopped following the audio.
                    // What it produced up to here is kept, and the reading will
                    // disagree with the other two, which is the refusal.
                    break;
                }
            } else {
                repeats = 0;
            }
            emitted.push(token as u32);
            probabilities.push(probability);
            ids.push(token);
        }
        let raw = self
            .tokenizer
            .decode(&emitted, true)
            .map_err(|e| format!("cannot read the transcript back: {e}"))?;
        let mean = if probabilities.is_empty() {
            0.0
        } else {
            probabilities.iter().sum::<f32>() / probabilities.len() as f32
        };
        Ok((raw, mean))
    }

    /// One decoder pass, answering the distribution over the next token.
    fn step(&mut self, ids: &[i64], hidden: &Hidden) -> Result<Vec<f32>, String> {
        let tokens = Tensor::from_array(([1, ids.len()], ids.to_vec()))
            .map_err(|e| format!("cannot build the token tensor: {e}"))?;
        let states = Tensor::from_array((
            [1, hidden.frames, hidden.width],
            hidden.data.clone(),
        ))
        .map_err(|e| format!("cannot build the encoder state tensor: {e}"))?;
        let outputs = self
            .decoder
            .run(ort::inputs![
                "input_ids" => tokens,
                "encoder_hidden_states" => states,
            ])
            .map_err(|e| format!("the transcriber refused the transcript so far: {e}"))?;
        let (shape, data) = first_tensor(&outputs)?;
        if shape.len() != 3 || shape[0] != 1 || shape[1] != ids.len() {
            return Err(format!(
                "the decoder answered a shape this code cannot read: {shape:?} for {} tokens",
                ids.len()
            ));
        }
        let vocab = shape[2];
        let last = (shape[1] - 1) * vocab;
        Ok(data[last..last + vocab].to_vec())
    }
}

/// The encoder's output, kept flat with its own shape.
struct Hidden {
    frames: usize,
    width: usize,
    data: Vec<f32>,
}

/// Open one graph, naming what a caller has to fix when the runtime is missing.
fn open(path: &Path) -> Result<Session, String> {
    let fail = |e: String| {
        format!(
            "cannot open {}: {e}. The onnxruntime library is loaded at run time; \
             set ORT_DYLIB_PATH to a libonnxruntime.so this build can use",
            path.display()
        )
    };
    let builder = Session::builder().map_err(|e| fail(e.to_string()))?;
    // Two threads, as with the detector: a helper that grabs every core makes the
    // page it is solving for slower.
    let mut builder = builder
        .with_intra_threads(2)
        .map_err(|e| fail(e.to_string()))?;
    builder.commit_from_file(path).map_err(|e| fail(e.to_string()))
}

/// The clip with silence around it and a floor of room tone under it.
///
/// The silence is so the first syllable is not the first frame. The room tone is
/// because a digitally silent pause is not a pause any microphone records: measured
/// on a synthesized clip of `94455`, the two fives were merged into one when the gaps
/// between them held exact zeros, and read as two when the same clip carried a
/// vendor's own hiss. The level is far below speech and the sequence is fixed, so a
/// transcript is still reproducible.
#[must_use]
fn framed(samples: &[f32]) -> Vec<f32> {
    let lead = (LEAD_S * f64::from(sound::RATE)) as usize;
    let tail = (TAIL_S * f64::from(sound::RATE)) as usize;
    let mut out = vec![0.0f32; lead + samples.len() + tail];
    out[lead..lead + samples.len()].copy_from_slice(samples);
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for slot in &mut out {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let noise = ((state >> 40) as f32 / 8_388_608.0) - 1.0;
        *slot += ROOM_TONE * noise;
    }
    out
}

/// The first float tensor a session answered, with its shape.
///
/// By position rather than by name: the encoder of one export calls its output
/// `last_hidden_state` and another calls it `hidden_states`, and both have exactly
/// one output.
fn first_tensor(outputs: &SessionOutputs<'_>) -> Result<(Vec<usize>, Vec<f32>), String> {
    let (name, value) = outputs
        .iter()
        .next()
        .ok_or_else(|| "the model answered no tensor at all".to_string())?;
    let (shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    Ok((shape.iter().map(|d| *d as usize).collect(), data.to_vec()))
}

/// The strongest token among the ones that may be emitted, and its share.
///
/// The share is taken over the allowed set, not the whole vocabulary: the question
/// is which answer this is, and a model that spent most of its mass on punctuation
/// it is not allowed to emit has still made a clear choice between the answers.
fn pick(logits: &[f32], allowed: Option<&[i64]>, end: i64) -> (i64, f32) {
    let mut candidates: Vec<i64> = match allowed {
        Some(ids) => {
            let mut ids = ids.to_vec();
            ids.push(end);
            ids
        }
        None => (0..logits.len() as i64).collect(),
    };
    candidates.retain(|id| (*id as usize) < logits.len());
    if candidates.is_empty() {
        return (end, 0.0);
    }
    let peak = candidates
        .iter()
        .map(|id| logits[*id as usize])
        .fold(f32::NEG_INFINITY, f32::max);
    let mut total = 0.0f32;
    let mut best = (end, f32::NEG_INFINITY);
    for id in &candidates {
        let value = (logits[*id as usize] - peak).exp();
        total += value;
        if value > best.1 {
            best = (*id, value);
        }
    }
    (best.0, if total > 0.0 { best.1 / total } else { 0.0 })
}

/// The characters of `raw` that the alphabet admits, in order.
///
/// The model writes what it heard, including the spacing a reader would put
/// between spoken digits. What the widget wants typed is the answer, so an
/// alphabet that holds no space drops them rather than typing them into a field
/// that would then hold `4 7 2`.
#[must_use]
pub fn spell(raw: &str, alphabet: &str) -> String {
    if alphabet.is_empty() {
        return raw.trim().to_string();
    }
    let wanted: BTreeSet<char> = alphabet.chars().collect();
    raw.chars().filter(|c| wanted.contains(c)).collect()
}

/// Fold three readings into one answer, weighted by how many agreed.
fn vote(readings: Vec<(String, String, f32)>) -> Transcript {
    let heard: Vec<String> = readings.iter().map(|(text, _, _)| text.clone()).collect();
    // The most agreed-upon non-empty answer. An empty reading is a reading that
    // heard nothing, and three of those is a refusal rather than an answer of "".
    let mut best: Option<(String, usize, f32)> = None;
    for (text, _, _) in &readings {
        if text.is_empty() {
            continue;
        }
        let agreed: Vec<f32> = readings
            .iter()
            .filter(|(other, _, _)| other == text)
            .map(|(_, _, mean)| *mean)
            .collect();
        let mean = agreed.iter().sum::<f32>() / agreed.len() as f32;
        let count = agreed.len();
        let better = match &best {
            None => true,
            Some((_, top_count, top_mean)) => {
                count > *top_count || (count == *top_count && mean > *top_mean)
            }
        };
        if better {
            best = Some((text.clone(), count, mean));
        }
    }
    let Some((text, agreement, mean)) = best else {
        return Transcript {
            text: String::new(),
            raw: readings
                .first()
                .map(|(_, raw, _)| raw.clone())
                .unwrap_or_default(),
            confidence: 0.0,
            agreement: 0,
            heard,
        };
    };
    let raw = readings
        .iter()
        .find(|(candidate, _, _)| *candidate == text)
        .map(|(_, raw, _)| raw.clone())
        .unwrap_or_default();
    let factor = AGREEMENT
        .get(agreement.min(AGREEMENT.len() - 1))
        .copied()
        .unwrap_or(1.0);
    Transcript {
        text,
        raw,
        confidence: mean * factor,
        agreement,
        heard,
    }
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
        let Err(error) = Listener::load(Path::new("/nonexistent/audio/model")) else {
            panic!("a directory with no model loaded");
        };
        assert!(error.contains("encoder_model.onnx"), "{error}");
        assert!(error.contains("decoder_model.onnx"), "{error}");
        assert!(error.contains(MODEL_ENV), "{error}");
        assert!(
            error.contains("refused rather than guessed"),
            "the refusal has to say what happens without a model: {error}"
        );
    }

    #[test]
    fn an_alphabet_keeps_only_the_characters_it_names() {
        assert_eq!(spell("4 7 2 9 1", "0123456789"), "47291");
        assert_eq!(spell(" 4-7-2 ", "0123456789"), "472");
        // No alphabet is an unmasked answer, trimmed and otherwise untouched.
        assert_eq!(spell("  hello there ", ""), "hello there");
        // A letter alphabet keeps the letters it names and drops everything else,
        // including the capital a model puts at the front of a sentence: a widget
        // whose answer is lowercase gets lowercase typed into it.
        assert_eq!(spell("Hello, there.", "abcdefghijklmnopqrstuvwxyz"), "ellothere");
    }

    #[test]
    fn a_masked_pick_chooses_among_the_allowed_tokens_only() {
        // Token 5 has the largest logit of all, but it is not allowed, so the
        // answer is the best of 1 and 2 and the share is taken between them.
        let logits = vec![0.0, 1.0, 2.0, 0.0, 0.0, 9.0];
        let (token, share) = pick(&logits, Some(&[1, 2]), 3);
        assert_eq!(token, 2);
        // exp(2)/(exp(2)+exp(1)+exp(0)) with the end token in the set.
        let expected = 2f32.exp() / (2f32.exp() + 1f32.exp() + 1.0);
        assert!((share - expected).abs() < 1e-5, "share is {share}");
        // Unmasked, the same logits answer the token nothing was masking.
        let (token, _) = pick(&logits, None, 3);
        assert_eq!(token, 5);
    }

    #[test]
    fn a_pick_with_nothing_allowed_ends_the_transcript() {
        let (token, share) = pick(&[1.0, 2.0], Some(&[]), 1);
        assert_eq!(token, 1);
        assert!(share > 0.0);
        // Ids past the end of the distribution are dropped rather than indexed.
        let (token, share) = pick(&[1.0, 2.0], Some(&[99]), 7);
        assert_eq!(token, 7, "an out of range id is not a token");
        assert_eq!(share, 0.0);
    }

    #[test]
    fn three_readings_that_agree_are_worth_more_than_two() {
        let unanimous = vote(vec![
            ("47291".to_string(), "4 7 2 9 1".to_string(), 0.9),
            ("47291".to_string(), "4 7 2 9 1".to_string(), 0.9),
            ("47291".to_string(), "4 7 2 9 1".to_string(), 0.9),
        ]);
        assert_eq!(unanimous.text, "47291");
        assert_eq!(unanimous.agreement, 3);
        assert!((unanimous.confidence - 0.9).abs() < 1e-6);

        let split = vote(vec![
            ("47291".to_string(), "4 7 2 9 1".to_string(), 0.9),
            ("47291".to_string(), "4 7 2 9 1".to_string(), 0.9),
            ("4729".to_string(), "4 7 2 9".to_string(), 0.95),
        ]);
        assert_eq!(split.text, "47291", "the majority reading is the answer");
        assert_eq!(split.agreement, 2);
        assert!(
            split.confidence < unanimous.confidence,
            "a split reading has to be worth less: {} vs {}",
            split.confidence,
            unanimous.confidence
        );
        assert_eq!(split.heard.len(), 3, "every reading is recorded");
    }

    #[test]
    fn three_readings_that_all_disagree_answer_the_surest_one_and_say_so() {
        let lone = vote(vec![
            ("111".to_string(), "1 1 1".to_string(), 0.5),
            ("222".to_string(), "2 2 2".to_string(), 0.8),
            ("333".to_string(), "3 3 3".to_string(), 0.4),
        ]);
        assert_eq!(lone.text, "222");
        assert_eq!(lone.agreement, 1);
        // 0.8 heard once is worth less than 0.8 heard twice, so a floor a caller
        // sets on the number alone still refuses this.
        assert!((lone.confidence - 0.8 * AGREEMENT[1]).abs() < 1e-6);
    }

    #[test]
    fn nothing_heard_is_not_an_empty_answer() {
        let silence = vote(vec![
            (String::new(), String::new(), 0.0),
            (String::new(), String::new(), 0.0),
            (String::new(), String::new(), 0.0),
        ]);
        assert!(silence.text.is_empty());
        assert_eq!(silence.agreement, 0);
        assert_eq!(silence.confidence, 0.0);
    }

    #[test]
    fn one_reading_out_of_three_being_empty_does_not_win() {
        // Two readings heard the same code and one heard nothing. The answer is the
        // code, at two thirds' worth of confidence, not an empty transcript.
        let answer = vote(vec![
            ("8143".to_string(), "8 1 4 3".to_string(), 0.88),
            (String::new(), String::new(), 0.0),
            ("8143".to_string(), "8 1 4 3".to_string(), 0.84),
        ]);
        assert_eq!(answer.text, "8143");
        assert_eq!(answer.agreement, 2);
        assert!((answer.confidence - 0.86 * AGREEMENT[2]).abs() < 1e-3);
    }

    #[test]
    fn the_speeds_are_a_reading_at_rate_and_one_either_side() {
        // The vote is only independent if the three readings are actually different
        // clips, and a rate that drifted to one side would make two of them nearly
        // the same reading.
        assert_eq!(SPEEDS.len(), 3);
        assert!(SPEEDS.contains(&1.0));
        let slower = SPEEDS.iter().filter(|s| **s > 1.0).count();
        let faster = SPEEDS.iter().filter(|s| **s < 1.0).count();
        assert_eq!((slower, faster), (1, 1), "the speeds are not either side of one");
    }
}
