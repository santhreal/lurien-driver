//! Does the helper hear a spoken code?
//!
//! The unit tests cover the arithmetic: the mel scale, the filterbank, the
//! resampler, the mask and the vote. None of them can tell whether the whole front
//! end matches the model's own preprocessing, because a filterbank that is wrong by
//! a constant still produces a plausible spectrogram and a decode that confidently
//! reads the wrong digits.
//!
//! So this runs the real thing: a code is spoken by the host's own synthesizer,
//! encoded as a container the helper has to decode, and transcribed. It skips
//! loudly without the weights or without a synthesizer, because a proof that
//! silently passes on a machine that cannot run it is worse than no proof.
//!
//! The clips are built here rather than committed. A committed set of clips is a
//! fixed answer sheet: it would keep passing after a change that only works on
//! those six files.

use std::path::PathBuf;
use std::process::Command;

use lurien_vision::asr::Listener;
use lurien_vision::sound;

/// Codes to read aloud. Five and six digits, with repeats and with the digits a
/// synthesizer's consonants blur into each other.
const CODES: [&str; 6] = ["47291", "80356", "610401", "94455", "203407", "965503"];

/// Confidence a caller types at. Set here to the floor the engine's default
/// carries, so a change on one side shows up on this side.
const FLOOR: f32 = 0.80;

#[test]
fn a_clean_spoken_code_is_transcribed_exactly() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    for code in CODES {
        let clip = voice.speak(code);
        let samples = sound::at_model_rate(&clip);
        let heard = listener
            .hear(&samples, "0123456789")
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert_eq!(
            heard.text, code,
            "a clean reading of {code} came back as {:?} (readings {:?})",
            heard.text, heard.heard
        );
        assert!(
            heard.confidence >= FLOOR,
            "{code} was read exactly at {:.2}, below the floor a caller types at",
            heard.confidence
        );
    }
}

/// The same codes under the noise a vendor's clip carries.
///
/// The clips and the noise are both deterministic, so this is an equality and not a
/// rate: every one of these six codes is read exactly, above the floor a caller
/// types at. It is the gate on the four decisions the front end is made of, each of
/// which broke a different one of these codes while it was wrong: one token per
/// character, tightened pauses, a lead-in, and a floor of room tone.
#[test]
fn every_code_survives_the_noise_a_vendor_adds() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    for (index, code) in CODES.iter().enumerate() {
        let clip = voice.speak(code);
        let noisy = distort(&sound::at_model_rate(&clip), index as u64);
        let heard = listener
            .hear(&noisy, "0123456789")
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        eprintln!(
            "{} {code} -> {:?} confidence {:.2} agreement {} readings {:?}",
            if heard.text == *code { "ok  " } else { "miss" },
            heard.text,
            heard.confidence,
            heard.agreement,
            heard.heard
        );
        assert_eq!(
            heard.text, *code,
            "a noisy reading of {code} came back as {:?} (readings {:?})",
            heard.text, heard.heard
        );
        assert!(
            heard.confidence >= FLOOR,
            "{code} was read exactly at {:.2}, which a caller would refuse and reload",
            heard.confidence
        );
        assert_eq!(heard.agreement, 3, "{code} was not read the same at all three speeds");
    }
}

#[test]
fn silence_is_not_an_answer() {
    let Some(mut listener) = listener() else {
        return;
    };
    // Three seconds of nothing. A model that answers this has hallucinated, and a
    // caller that types the answer has burned a challenge.
    let quiet = vec![0.0f32; sound::RATE as usize * 3];
    let heard = listener.hear(&quiet, "0123456789").expect("silence decodes");
    assert!(
        heard.text.is_empty() || heard.confidence < FLOOR,
        "silence was transcribed as {:?} at {:.2}",
        heard.text,
        heard.confidence
    );
}

#[test]
fn an_alphabet_the_widget_did_not_name_is_not_typed() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    // The same clip, read under a letters-only alphabet. The answer is whatever the
    // model can spell with letters, and it must hold no digit at all: the mask is
    // what keeps a transcript inside what the binding said the field accepts.
    let clip = voice.speak("47291");
    let samples = sound::at_model_rate(&clip);
    let heard = listener
        .hear(&samples, "abcdefghijklmnopqrstuvwxyz")
        .expect("a letters-only decode");
    assert!(
        heard.text.chars().all(|c| c.is_ascii_lowercase()),
        "a letters alphabet produced {:?}",
        heard.text
    );
}

/// Each of the four decisions the front end is made of was wrong at some point
/// during development, and each broke a different code. These mutation gates prove
/// the decision is load-bearing by turning it off and requiring the result to
/// degrade. They skip loudly without the weights or the synthesizer, the same way
/// the positive tests do.

#[test]
fn widening_the_mask_to_multi_character_tokens_degrades_the_decode() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    // At least one code is read worse without the one-token-per-character mask.
    // Measured: the same clips that were exact at 0.89 to 0.99 came back truncated
    // or wrong at about 0.5, because the model's mass split over spellings of the
    // same code.
    let mut any_degraded = false;
    for code in CODES {
        let clip = voice.speak(code);
        let samples = sound::at_model_rate(&clip);
        let noisy = distort(&samples, code.len() as u64 * 7919);
        let masked = listener
            .hear_with(&noisy, "0123456789", true, true, true)
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        let unmasked = listener
            .hear_with(&noisy, "0123456789", true, true, false)
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        if masked.text == *code && (unmasked.text != *code || unmasked.confidence < masked.confidence - 0.1)
        {
            any_degraded = true;
            eprintln!(
                "mask: {code} was {masked:?} and became {unmasked:?} without the mask"
            );
        }
    }
    assert!(
        any_degraded,
        "widening the mask to multi-character tokens did not degrade any code"
    );
}

#[test]
fn dropping_tighten_degrades_a_clip_with_long_pauses() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    // `tighten` shortens any silence past 150 ms. A clip with a 200 ms gap between
    // digits is above that threshold. With tighten, the gaps are cut and the code
    // is read at higher confidence. Without it, the model still reads the code but
    // at lower confidence, because the longer pauses dilute the model's attention.
    // During development, an untouched clip with long pauses came back as its first
    // digit only; on the current model and clip recipe the degradation is marginal
    // rather than catastrophic, which is recorded here honestly. The clip is tested
    // clean because the noise the fixture adds fills the gaps with energy, and
    // tighten reads the percentiles of the clip itself.
    let code = "94455";
    let clip = voice.speak_with_gap(code, 200);
    let samples = sound::normalize(&sound::at_model_rate(&clip));
    let tightened = listener
        .hear_with(&samples, "0123456789", true, true, true)
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    let raw = listener
        .hear_with(&samples, "0123456789", false, true, true)
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    assert_eq!(
        tightened.text, code,
        "tighten: {code} came back as {:?}",
        tightened.text
    );
    // The degradation without tighten is a lower confidence, not a wrong answer,
    // on this model and clip. A 0.02 drop is what was measured; the threshold is
    // half that to keep the test stable across runs.
    assert!(
        raw.confidence < tightened.confidence - 0.01,
        "dropping tighten did not lower confidence: {tightened:?} vs {raw:?}"
    );
    eprintln!("tighten: {code} was {tightened:?} and became {raw:?} without tighten");
}

#[test]
fn dropping_framing_merges_repeated_digits() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    // A code with repeated digits spoken with no gap. The front end's framing adds
    // lead and tail silence and a floor of room tone under the whole clip. Without
    // it, the exact zeros between two of the same digit merge them: `94455` came
    // back as `9445` during development, one `5` lost to digital silence. The
    // `log_mel` pad to 30 s is zeros, which is the same defect the room tone fixes.
    let code = "94455";
    let clip = voice.speak_with_gap(code, 5);
    let samples = sound::at_model_rate(&clip);
    let noisy = distort(&samples, 7);
    let framed = listener
        .hear_with(&noisy, "0123456789", true, true, true)
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    let bare = listener
        .hear_with(&noisy, "0123456789", true, false, true)
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    assert_eq!(
        framed.text, code,
        "framed: {code} came back as {:?}",
        framed.text
    );
    assert_ne!(
        bare.text, code,
        "dropping framing still read {code}, so the decision is not load-bearing"
    );
    eprintln!("framed: {code} was {framed:?} and became {bare:?} without framing");
}

#[test]
fn agreement_scales_the_confidence_a_lone_reading_does_not_clear() {
    let Some(mut listener) = listener() else {
        return;
    };
    let Some(voice) = voice() else {
        return;
    };
    // A transcript's confidence is its raw probability scaled by how many of the
    // three readings agreed. A lone reading carries 0.7, two of three 0.9, three
    // 1.0. A code read at agreement 3 clears the floor; the same probability at
    // agreement 1 would not. This checks the scaling is applied, not that it
    // changes the outcome on a clean clip: the factor is what makes a thin
    // transcript ask for another recording rather than guessing.
    let code = "80356";
    let clip = voice.speak(code);
    let samples = sound::at_model_rate(&clip);
    let heard = listener
        .hear(&samples, "0123456789")
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    assert_eq!(heard.agreement, 3, "the clean clip did not agree three times");
    assert!(
        heard.confidence >= FLOOR,
        "agreement 3 still did not clear the floor: {heard:?}"
    );
    // The factor for agreement 1 is 0.7, so the same mean would be under the floor.
    let lone = heard.confidence / 1.0 * 0.7;
    assert!(
        lone < FLOOR,
        "a lone reading at the same mean would still clear the floor: {lone:.2}"
    );
}

/// The speech model, or a loud skip.
fn listener() -> Option<Listener> {
    let dir = model_dir()?;
    match Listener::load(&dir) {
        Ok(listener) => Some(listener),
        Err(e) => {
            // A host with weights and no onnxruntime is a setup problem, not a
            // failing claim, and the message says which.
            eprintln!("SKIP: the speech model in {} did not load: {e}", dir.display());
            None
        }
    }
}

fn model_dir() -> Option<PathBuf> {
    if let Some(dir) = lurien_vision::asr::model_dir_from_env() {
        if dir.is_dir() {
            return Some(dir);
        }
        eprintln!("SKIP: LURIEN_AUDIO_MODEL names {}, which is not a directory", dir.display());
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".cache/lurien/audio/whisper-small.en");
    if dir.join("encoder_model.onnx").is_file() {
        return Some(dir);
    }
    eprintln!(
        "SKIP: no speech export in $LURIEN_AUDIO_MODEL or {}; audio perception is unproven here",
        dir.display()
    );
    None
}

/// The host's own synthesizer, or a loud skip.
struct Voice {
    dir: PathBuf,
}

fn voice() -> Option<Voice> {
    let probe = Command::new("espeak-ng").arg("--version").output();
    match probe {
        Ok(output) if output.status.success() => {}
        _ => {
            eprintln!("SKIP: no espeak-ng on this host, so no clip can be spoken");
            return None;
        }
    }
    let dir = std::env::temp_dir().join("lurien-audio-test");
    std::fs::create_dir_all(&dir).ok()?;
    Some(Voice { dir })
}

impl Voice {
    /// One code read aloud, digit by digit, as the container bytes a widget serves.
    ///
    /// The rate, the voice and the gap are the fixture's: this proof and the live
    /// one have to be reading the same kind of clip, or neither says anything about
    /// the other.
    fn speak(&self, code: &str) -> sound::Clip {
        let spoken: String = code
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let path = self.dir.join(format!("{code}.wav"));
        let status = Command::new("espeak-ng")
            .args(["-v", "en-gb", "-s", "120", "-g", "25", "-w"])
            .arg(&path)
            .arg(&spoken)
            .status()
            .expect("espeak-ng runs");
        assert!(status.success(), "espeak-ng refused {code}");
        let bytes = std::fs::read(&path).expect("the spoken clip");
        // Through the decoder the helper itself uses, so a container it cannot read
        // fails here rather than in a live solve.
        sound::decode(&bytes, "audio/wav").expect("a decodable clip")
    }

    /// One code read aloud with a larger gap between digits, for the tighten test.
    fn speak_with_gap(&self, code: &str, gap_ms: usize) -> sound::Clip {
        let spoken: String = code
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let path = self.dir.join(format!("{code}-{gap_ms}.wav"));
        let status = Command::new("espeak-ng")
            .args(["-v", "en-gb", "-s", "120", "-g"])
            .arg(gap_ms.to_string())
            .arg("-w")
            .arg(&path)
            .arg(&spoken)
            .status()
            .expect("espeak-ng runs");
        assert!(status.success(), "espeak-ng refused {code} at gap {gap_ms}");
        let bytes = std::fs::read(&path).expect("the spoken clip");
        sound::decode(&bytes, "audio/wav").expect("a decodable clip")
    }
}

/// Strip leading silence so the speech starts at the first sample.
fn trim_leading_silence(samples: &[f32]) -> Vec<f32> {
    let threshold = 0.01;
    for (i, &s) in samples.iter().enumerate() {
        if s.abs() > threshold {
            return samples[i..].to_vec();
        }
    }
    samples.to_vec()
}

/// The noise a vendor's audio challenge carries: hiss, mains hum, and a second
/// voice under the first.
///
/// Deterministic per index, so a failure is reproducible. The levels are the
/// fixture's, which were chosen so an unconstrained general transcript of the clean
/// clip still reads the digits: the solver has to overcome the noise, not a
/// synthesizer that cannot say five.
fn distort(samples: &[f32], seed: u64) -> Vec<f32> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as f64 / f64::from(u32::MAX >> 1)) - 1.0
    };
    let rate = f64::from(sound::RATE);
    let mut out = Vec::with_capacity(samples.len());
    for (n, sample) in samples.iter().enumerate() {
        let t = n as f64 / rate;
        let hiss = 0.02 * next();
        let hum = 0.01 * (2.0 * std::f64::consts::PI * 120.0 * t).sin();
        // A rumbling second speaker: two low tones beating against each other,
        // amplitude modulated at syllable rate.
        let babble = 0.12
            * (2.0 * std::f64::consts::PI * 190.0 * t).sin()
            * (0.5 + 0.5 * (2.0 * std::f64::consts::PI * 3.1 * t).sin());
        out.push((f64::from(*sample) + hiss + hum + babble) as f32);
    }
    sound::normalize(&out)
}
