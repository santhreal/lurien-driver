//! What a widget serves, as the tensor a speech model expects.
//!
//! Three steps, none of them negotiable. A vendor serves MP3 at whatever rate its
//! encoder chose, so the bytes are decoded and mixed to one channel. A speech
//! model is trained at one rate, so the samples are resampled to it. And the model
//! does not take samples at all: it takes a log-mel spectrogram built with the
//! exact window, filterbank and scaling its own preprocessing used, because a
//! filterbank that disagrees with the training one shifts every frame and reads as
//! a model that is merely deaf.
//!
//! The numbers here are Whisper's: 16 kHz, 80 mel bands, a 400-sample window with
//! a 160-sample hop, and a 30 second input the encoder's shape is fixed to. They
//! are named rather than inlined so a different export is a constant change and
//! not a hunt.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::conv::IntoSample;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

/// Rate the model was trained at.
pub const RATE: u32 = 16_000;

/// Mel bands per frame.
pub const MELS: usize = 80;

/// Window length, in samples.
pub const WINDOW: usize = 400;

/// Hop between frames, in samples.
pub const HOP: usize = 160;

/// Frames the encoder's input is shaped for: 30 seconds at this hop.
pub const FRAMES: usize = 3_000;

/// Samples in that window.
pub const SAMPLES: usize = RATE as usize * 30;

/// Rows in a magnitude spectrum of a `WINDOW`-point real transform.
const BINS: usize = WINDOW / 2 + 1;

/// Highest frequency the filterbank covers: Nyquist at this rate.
const MEL_MAX_HZ: f64 = RATE as f64 / 2.0;

/// Longest clip that will be decoded, in samples at the source rate. A widget's
/// audio challenge is seconds long; anything past this is a stream, and decoding
/// it would spend the kind's whole budget filling memory.
const DECODE_MAX: usize = RATE as usize * 300;

/// Mono samples, and the rate they are at.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Samples in `-1..=1`, one channel.
    pub samples: Vec<f32>,
    /// Sample rate as the container declared it.
    pub rate: u32,
}

/// Decode container bytes to mono samples.
///
/// `mime` is a hint, not a contract: a vendor that serves MP3 as
/// `application/octet-stream` is normal, so the probe is allowed to disagree with
/// it. The extension is passed too, since that is what a probe recognizes.
///
/// # Errors
/// If no audio track can be found, if the codec is not in this build, or if the
/// stream is longer than a challenge could plausibly be.
pub fn decode(bytes: &[u8], mime: &str) -> Result<Clip, String> {
    if bytes.is_empty() {
        return Err("the widget's audio is zero bytes, so there is nothing to hear".to_string());
    }
    let source = Box::new(std::io::Cursor::new(bytes.to_vec()));
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = extension_for(mime) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &Default::default(), &Default::default())
        .map_err(|e| {
            format!(
                "the {} bytes the widget served are not audio this build reads ({mime}): {e}",
                bytes.len()
            )
        })?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "the audio the widget served carries no decodable track".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("no decoder for the codec the widget served: {e}"))?;
    let mut samples: Vec<f32> = Vec::new();
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // End of stream and a truncated tail are the same thing here: whatever
            // was decoded is what the widget played.
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("the widget's audio stopped decoding: {e}")),
        };
        if rate == 0 {
            rate = decoded.spec().rate;
        }
        mix_into(&decoded, &mut samples);
        if samples.len() > DECODE_MAX {
            return Err(format!(
                "the widget's audio is longer than {} seconds, which is a stream and not a challenge",
                DECODE_MAX / RATE as usize
            ));
        }
    }
    if samples.is_empty() {
        return Err("the widget's audio decoded to no samples".to_string());
    }
    if rate == 0 {
        return Err("the widget's audio declares no sample rate".to_string());
    }
    Ok(Clip { samples, rate })
}

/// The file extension a probe would recognize for a served MIME type.
fn extension_for(mime: &str) -> Option<&'static str> {
    let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match mime.as_str() {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" | "audio/x-mpeg-3" => Some("mp3"),
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => Some("wav"),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some("m4a"),
        _ => None,
    }
}

/// Average every channel of one decoded buffer onto the end of `out`.
///
/// A vendor's clip is usually mono already, but a stereo one whose channels are
/// taken as consecutive samples plays at half speed, which a model hears as a
/// different voice saying something else.
fn mix_into(buffer: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    macro_rules! mix {
        ($buf:expr) => {{
            let spec = $buf.spec();
            let channels = spec.channels.count().max(1);
            let frames = $buf.frames();
            out.reserve(frames);
            for frame in 0..frames {
                let mut sum = 0.0f32;
                for channel in 0..channels {
                    let sample: f32 = $buf.chan(channel)[frame].into_sample();
                    sum += sample;
                }
                out.push(sum / channels as f32);
            }
        }};
    }
    match buffer {
        AudioBufferRef::U8(buf) => mix!(buf),
        AudioBufferRef::U16(buf) => mix!(buf),
        AudioBufferRef::U24(buf) => mix!(buf),
        AudioBufferRef::U32(buf) => mix!(buf),
        AudioBufferRef::S8(buf) => mix!(buf),
        AudioBufferRef::S16(buf) => mix!(buf),
        AudioBufferRef::S24(buf) => mix!(buf),
        AudioBufferRef::S32(buf) => mix!(buf),
        AudioBufferRef::F32(buf) => mix!(buf),
        AudioBufferRef::F64(buf) => mix!(buf),
    }
}

/// Resample by a ratio: `2.0` returns twice as many samples for the same sound.
///
/// A windowed sinc, not a nearest neighbour: dropping or repeating samples folds
/// the aliases of every consonant back into the band the model reads. The cutoff
/// follows the ratio, so downsampling filters before it decimates, which is the
/// half of resampling that a naive implementation leaves out.
#[must_use]
pub fn resample_by(input: &[f32], ratio: f64) -> Vec<f32> {
    if input.is_empty() || !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }
    if (ratio - 1.0).abs() < f64::EPSILON {
        return input.to_vec();
    }
    let cutoff = 0.95 * ratio.min(1.0);
    // Support of the kernel in input samples: wider when the cutoff is lower, so
    // the transition band stays as narrow as the taps allow.
    let half = (16.0 / cutoff).ceil();
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for index in 0..out_len {
        let centre = index as f64 / ratio;
        let first = (centre - half).ceil() as i64;
        let last = (centre + half).floor() as i64;
        let mut sum = 0.0f64;
        let mut weight = 0.0f64;
        for n in first..=last {
            if n < 0 || n as usize >= input.len() {
                continue;
            }
            let t = n as f64 - centre;
            let w = sinc(cutoff * t) * blackman(t / half);
            sum += f64::from(input[n as usize]) * w;
            weight += w;
        }
        out.push(if weight.abs() > 1e-12 {
            (sum / weight) as f32
        } else {
            0.0
        });
    }
    out
}

/// Resample from one rate to another.
#[must_use]
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == 0 || to == 0 {
        return Vec::new();
    }
    resample_by(input, f64::from(to) / f64::from(from))
}

/// The clip at the model's rate, one channel.
#[must_use]
pub fn at_model_rate(clip: &Clip) -> Vec<f32> {
    if clip.rate == RATE {
        clip.samples.clone()
    } else {
        resample(&clip.samples, clip.rate, RATE)
    }
}

/// The same speech, `factor` times as long: `1.1` is a tenth slower.
///
/// Speaking rate is the axis a digit-by-digit clip is most fragile on, and the
/// same model hears a dropped digit at one rate and all of them at another. The
/// pitch moves with the rate, which is what makes the three readings independent
/// enough to be worth voting on.
#[must_use]
pub fn stretch(samples: &[f32], factor: f64) -> Vec<f32> {
    resample_by(samples, factor)
}

/// Peak-normalise, so a quiet clip and a loud one land on the same filterbank.
#[must_use]
pub fn normalize(samples: &[f32]) -> Vec<f32> {
    let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
    if peak <= 1e-6 {
        return samples.to_vec();
    }
    let gain = 0.95 / peak;
    samples.iter().map(|s| s * gain).collect()
}

/// Frame length the energy of a clip is measured over, in samples: 10 ms.
const ENERGY_FRAME: usize = RATE as usize / 100;

/// Shorten every silence longer than `max_gap_s` to that length.
///
/// A vendor reads an answer one character at a time with a long pause between each,
/// and a speech model trained on continuous speech treats a long pause as the end of
/// what was said: measured on spoken digit codes, an untouched clip came back as its
/// first digit and nothing else, three readings out of three. Tightening the pauses
/// keeps every character and the order they were said in, and takes away the only
/// cue that the recording had finished.
///
/// Silence is relative to this clip: a threshold a fraction of the way from its own
/// quietest frames to its loudest, so a noisy recording is not read as one long
/// word and a clean one is not cut into pieces.
#[must_use]
pub fn tighten(samples: &[f32], max_gap_s: f64) -> Vec<f32> {
    let max_gap = (max_gap_s * f64::from(RATE)) as usize;
    if samples.len() <= ENERGY_FRAME * 2 || max_gap == 0 {
        return samples.to_vec();
    }
    let frames: Vec<f32> = samples
        .chunks(ENERGY_FRAME)
        .map(|chunk| {
            let sum: f32 = chunk.iter().map(|s| s * s).sum();
            (sum / chunk.len() as f32).sqrt()
        })
        .collect();
    let mut sorted = frames.clone();
    sorted.sort_by(f32::total_cmp);
    let quiet = sorted[sorted.len() / 5];
    let loud = sorted[sorted.len() * 19 / 20];
    if loud <= quiet {
        return samples.to_vec();
    }
    let threshold = quiet + 0.15 * (loud - quiet);
    let mut out = Vec::with_capacity(samples.len());
    let mut run = 0usize;
    for (index, frame) in frames.iter().enumerate() {
        let start = index * ENERGY_FRAME;
        let end = ((index + 1) * ENERGY_FRAME).min(samples.len());
        if *frame > threshold {
            run = 0;
            out.extend_from_slice(&samples[start..end]);
            continue;
        }
        run += end - start;
        // The head of a silence is kept, so a word boundary is still audible; the
        // rest of it is what the model reads as an ending.
        if run <= max_gap {
            out.extend_from_slice(&samples[start..end]);
        }
    }
    if out.is_empty() {
        return samples.to_vec();
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    }
}

/// Blackman window over `-1..=1`, zero outside.
fn blackman(x: f64) -> f64 {
    if x.abs() >= 1.0 {
        return 0.0;
    }
    let t = (x + 1.0) / 2.0;
    let two_pi = 2.0 * std::f64::consts::PI;
    0.42 - 0.5 * (two_pi * t).cos() + 0.08 * (2.0 * two_pi * t).cos()
}

/// Hz to mel, on the scale the export's preprocessing uses: linear below a
/// kilohertz, logarithmic above it.
#[must_use]
pub fn hz_to_mel(hz: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if hz >= MIN_LOG_HZ {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / logstep
    } else {
        hz / F_SP
    }
}

/// Mel back to Hz.
#[must_use]
pub fn mel_to_hz(mel: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if mel >= MIN_LOG_MEL {
        MIN_LOG_HZ * ((mel - MIN_LOG_MEL) * logstep).exp()
    } else {
        mel * F_SP
    }
}

/// The front end: window, filterbank and transform, built once and reused.
///
/// Building the filterbank costs nothing next to a model pass, but the planner
/// wants to be asked once, and a helper answering a round of challenges runs this
/// several times per solve.
pub struct Frontend {
    /// `MELS` rows of `BINS` weights, row-major.
    filters: Vec<f32>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new()
    }
}

impl Frontend {
    /// The front end Whisper's preprocessing describes.
    #[must_use]
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        Self {
            filters: mel_filters(),
            window: hann(WINDOW),
            fft: planner.plan_fft_forward(WINDOW),
        }
    }

    /// The 30 second log-mel spectrogram of `samples`, row-major `MELS * FRAMES`.
    ///
    /// Shorter audio is zero padded to the window the encoder's shape declares and
    /// longer audio is cut, which is what the reference implementation does: the
    /// graph has no other input length.
    #[must_use]
    pub fn log_mel(&self, samples: &[f32]) -> Vec<f32> {
        // Pad or trim first, then reflect at the edges, in that order. Reflecting
        // the speech and then padding with zeros puts a copy of the first word
        // before the clip, and the decode starts on it.
        let mut audio = vec![0.0f32; SAMPLES];
        let take = samples.len().min(SAMPLES);
        audio[..take].copy_from_slice(&samples[..take]);
        let pad = WINDOW / 2;
        let mut padded = vec![0.0f32; SAMPLES + 2 * pad];
        for (i, slot) in padded.iter_mut().enumerate() {
            let source = i as i64 - pad as i64;
            let source = if source < 0 {
                (-source) as usize
            } else if source as usize >= SAMPLES {
                // Reflect around the last sample.
                2 * (SAMPLES - 1) - source as usize
            } else {
                source as usize
            };
            *slot = audio[source.min(SAMPLES - 1)];
        }
        let mut mel = vec![0.0f32; MELS * FRAMES];
        let mut scratch = vec![Complex32::new(0.0, 0.0); WINDOW];
        let mut power = vec![0.0f32; BINS];
        for frame in 0..FRAMES {
            let start = frame * HOP;
            for (i, slot) in scratch.iter_mut().enumerate() {
                let sample = padded.get(start + i).copied().unwrap_or(0.0);
                *slot = Complex32::new(sample * self.window[i], 0.0);
            }
            self.fft.process(&mut scratch);
            for (bin, slot) in power.iter_mut().enumerate() {
                *slot = scratch[bin].norm_sqr();
            }
            for band in 0..MELS {
                let row = &self.filters[band * BINS..(band + 1) * BINS];
                let mut sum = 0.0f32;
                for (weight, value) in row.iter().zip(power.iter()) {
                    sum += weight * value;
                }
                mel[band * FRAMES + frame] = sum.max(1e-10).log10();
            }
        }
        // The floor is relative to this clip's own loudest band, so a quiet
        // recording is not read as silence, and the shift is the export's.
        let peak = mel.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let floor = peak - 8.0;
        for value in &mut mel {
            *value = (value.max(floor) + 4.0) / 4.0;
        }
        mel
    }
}

/// Periodic Hann window, which is the one a spectrogram wants: the symmetric
/// window of the same length has a duplicated endpoint and leaks differently.
#[must_use]
pub fn hann(length: usize) -> Vec<f32> {
    (0..length)
        .map(|n| {
            let t = 2.0 * std::f64::consts::PI * n as f64 / length as f64;
            (0.5 - 0.5 * t.cos()) as f32
        })
        .collect()
}

/// `MELS` triangular filters over `BINS` spectrum rows, area-normalised.
#[must_use]
pub fn mel_filters() -> Vec<f32> {
    let mel_min = hz_to_mel(0.0);
    let mel_max = hz_to_mel(MEL_MAX_HZ);
    let edges: Vec<f64> = (0..MELS + 2)
        .map(|i| {
            let mel = mel_min + (mel_max - mel_min) * i as f64 / (MELS + 1) as f64;
            mel_to_hz(mel)
        })
        .collect();
    let bin_hz: Vec<f64> = (0..BINS)
        .map(|bin| bin as f64 * f64::from(RATE) / WINDOW as f64)
        .collect();
    let mut filters = vec![0.0f32; MELS * BINS];
    for band in 0..MELS {
        let (low, centre, high) = (edges[band], edges[band + 1], edges[band + 2]);
        // Two triangles of the same height give the higher bands more energy than
        // the lower ones purely because they are wider. The area normalisation is
        // part of the training preprocessing, not a taste.
        let area = 2.0 / (high - low);
        for (bin, &hz) in bin_hz.iter().enumerate() {
            let rising = (hz - low) / (centre - low);
            let falling = (high - hz) / (high - centre);
            let weight = rising.min(falling).max(0.0) * area;
            filters[band * BINS + bin] = weight as f32;
        }
    }
    filters
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone at a known frequency, for the tests that need real spectral content.
    fn tone(hz: f64, seconds: f64, rate: u32) -> Vec<f32> {
        let count = (seconds * f64::from(rate)) as usize;
        (0..count)
            .map(|n| {
                let t = n as f64 / f64::from(rate);
                (2.0 * std::f64::consts::PI * hz * t).sin() as f32 * 0.5
            })
            .collect()
    }

    #[test]
    fn a_mel_scale_is_linear_below_a_kilohertz_and_logarithmic_above_it() {
        // The break is what makes it a mel scale rather than a log: below it, twice
        // the frequency is twice the mel.
        assert!((hz_to_mel(200.0) * 2.0 - hz_to_mel(400.0)).abs() < 1e-9);
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-9);
        // Above it, equal ratios are equal steps.
        let step = hz_to_mel(4000.0) - hz_to_mel(2000.0);
        assert!((hz_to_mel(2000.0) - hz_to_mel(1000.0) - step).abs() < 1e-6);
        for hz in [0.0, 120.0, 999.9, 1000.0, 3000.0, 8000.0] {
            assert!((mel_to_hz(hz_to_mel(hz)) - hz).abs() < 1e-6, "{hz} did not round trip");
        }
    }

    #[test]
    fn every_mel_filter_covers_a_band_above_the_one_below_it() {
        let filters = mel_filters();
        assert_eq!(filters.len(), MELS * BINS);
        let mut previous = 0usize;
        for band in 0..MELS {
            let row = &filters[band * BINS..(band + 1) * BINS];
            let peak = row
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).expect("finite"))
                .expect("a row");
            assert!(*peak.1 > 0.0, "band {band} is empty");
            assert!(
                peak.0 >= previous,
                "band {band} peaks below band {}: {} then {}",
                band - 1,
                previous,
                peak.0
            );
            previous = peak.0;
            assert!(
                row.iter().all(|w| *w >= 0.0),
                "band {band} holds a negative weight"
            );
        }
    }

    #[test]
    fn a_tone_lands_in_the_band_that_covers_its_frequency() {
        // The front end is only useful if energy ends up where the frequency is.
        // A filterbank built on the wrong scale still produces a plausible-looking
        // spectrogram, so this checks the mapping and not merely the shape.
        let front = Frontend::new();
        let mel = front.log_mel(&tone(1000.0, 1.0, RATE));
        let mid = FRAMES / 100;
        let column: Vec<f32> = (0..MELS).map(|band| mel[band * FRAMES + mid]).collect();
        let loudest = column
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).expect("finite"))
            .expect("a column")
            .0;
        let edges: Vec<f64> = (0..MELS + 2)
            .map(|i| {
                let mel_min = hz_to_mel(0.0);
                let mel_max = hz_to_mel(MEL_MAX_HZ);
                mel_to_hz(mel_min + (mel_max - mel_min) * i as f64 / (MELS + 1) as f64)
            })
            .collect();
        let low = edges[loudest];
        let high = edges[loudest + 2];
        assert!(
            (low..=high).contains(&1000.0),
            "a 1 kHz tone peaked in band {loudest}, which covers {low:.0}..{high:.0} Hz"
        );
    }

    #[test]
    fn silence_and_a_tone_do_not_produce_the_same_frames() {
        let front = Frontend::new();
        let quiet = front.log_mel(&vec![0.0f32; RATE as usize]);
        let loud = front.log_mel(&tone(440.0, 1.0, RATE));
        assert_eq!(quiet.len(), MELS * FRAMES);
        assert_ne!(quiet, loud);
        // Silence is uniform: every band is at the relative floor.
        let first = quiet[0];
        assert!(
            quiet.iter().all(|v| (v - first).abs() < 1e-6),
            "silence produced structure"
        );
    }

    #[test]
    fn a_clip_longer_than_the_window_is_cut_rather_than_wrapped() {
        let front = Frontend::new();
        // Two different sounds, the second past the 30 second window. The frames
        // must not carry it: an encoder input that wrapped would answer with words
        // from the wrong end of a long stream.
        let mut long = tone(440.0, 30.0, RATE);
        long.extend(tone(1500.0, 5.0, RATE));
        let cut = front.log_mel(&long);
        let just = front.log_mel(&tone(440.0, 30.0, RATE));
        assert_eq!(cut, just);
    }

    #[test]
    fn resampling_preserves_a_tone_rather_than_its_alias() {
        // 3 kHz sampled down to 8 kHz stays 3 kHz; a decimation with no filter
        // folds it to 1 kHz, which is the classic silent failure.
        let source = tone(3000.0, 0.5, RATE);
        let down = resample(&source, RATE, 8_000);
        assert!(
            (down.len() as i64 - (source.len() / 2) as i64).abs() <= 2,
            "8 kHz of half a second is not {} samples",
            down.len()
        );
        let power_at = |samples: &[f32], rate: u32, hz: f64| -> f64 {
            let mut re = 0.0;
            let mut im = 0.0;
            for (n, s) in samples.iter().enumerate() {
                let t = 2.0 * std::f64::consts::PI * hz * n as f64 / f64::from(rate);
                re += f64::from(*s) * t.cos();
                im += f64::from(*s) * t.sin();
            }
            (re * re + im * im).sqrt() / samples.len() as f64
        };
        let kept = power_at(&down, 8_000, 3000.0);
        let alias = power_at(&down, 8_000, 1000.0);
        assert!(kept > 0.1, "the tone did not survive: {kept}");
        assert!(alias < kept / 10.0, "an alias appeared at 1 kHz: {alias} vs {kept}");
    }

    #[test]
    fn stretching_changes_the_length_and_keeps_the_speech() {
        let source = tone(500.0, 1.0, RATE);
        let slow = stretch(&source, 1.1);
        assert!(
            (slow.len() as f64 - source.len() as f64 * 1.1).abs() < 4.0,
            "a tenth slower is not {} samples",
            slow.len()
        );
        let fast = stretch(&source, 0.9);
        assert!(fast.len() < source.len());
        // A stretch is not a fade: the amplitude survives.
        let peak = slow.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.4, "the stretched clip lost its level: {peak}");
    }

    #[test]
    fn normalising_lifts_a_quiet_clip_and_leaves_silence_alone() {
        let quiet: Vec<f32> = tone(440.0, 0.1, RATE).iter().map(|s| s * 0.01).collect();
        let lifted = normalize(&quiet);
        let peak = lifted.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!((peak - 0.95).abs() < 1e-3, "peak is {peak}");
        let silence = vec![0.0f32; 100];
        assert_eq!(normalize(&silence), silence);
    }

    #[test]
    fn a_wav_body_decodes_to_the_samples_it_carries() {
        // Written here rather than committed: the point is that a container the
        // helper is handed round trips, and a fixture file would also pass if the
        // decoder silently returned the wrong channel count.
        let rate = 22_050u32;
        let source = tone(600.0, 0.25, rate);
        let bytes = wav_bytes(&source, rate);
        let clip = decode(&bytes, "audio/wav").expect("a wav clip");
        assert_eq!(clip.rate, rate);
        assert_eq!(clip.samples.len(), source.len());
        for (decoded, original) in clip.samples.iter().zip(source.iter()) {
            assert!(
                (decoded - original).abs() < 2e-4,
                "sample drifted: {decoded} vs {original}"
            );
        }
        let at_rate = at_model_rate(&clip);
        assert!(
            (at_rate.len() as i64 - (RATE as f64 * 0.25) as i64).abs() <= 2,
            "resampled length is {}",
            at_rate.len()
        );
    }

    #[test]
    fn a_stereo_wav_is_mixed_rather_than_read_as_twice_the_speech() {
        let rate = 16_000u32;
        let left = tone(600.0, 0.2, rate);
        let mut interleaved = Vec::with_capacity(left.len() * 2);
        for sample in &left {
            interleaved.push(*sample);
            interleaved.push(*sample);
        }
        let bytes = wav_bytes_channels(&interleaved, rate, 2);
        let clip = decode(&bytes, "audio/wav").expect("a stereo wav");
        assert_eq!(
            clip.samples.len(),
            left.len(),
            "a stereo clip decoded to twice its length, so it would play at half speed"
        );
    }

    #[test]
    fn bytes_that_are_not_audio_are_refused_by_name() {
        let error = decode(b"", "audio/wav").expect_err("empty is not audio");
        assert!(error.contains("zero bytes"), "{error}");
        let error = decode(b"<html>not audio</html>", "audio/mpeg").expect_err("html is not audio");
        assert!(
            error.contains("not audio this build reads"),
            "a page served instead of a clip has to say so: {error}"
        );
    }

    /// 16-bit PCM WAV bytes for mono samples.
    fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
        wav_bytes_channels(samples, rate, 1)
    }

    fn wav_bytes_channels(samples: &[f32], rate: u32, channels: u16) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| {
                let value = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                value.to_le_bytes()
            })
            .collect();
        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }
}
