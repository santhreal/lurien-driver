//! The grid: which cells answer the question the widget asked.
//!
//! The browser owns geometry. It knows where the cells are, because it laid them
//! out, and it sends their rectangles with the crop. This file decides which of
//! them match the phrase, and nothing here knows what a browsing context is.
//!
//! A raw similarity is not a decision: cosine scores drift with the phrasing and
//! with the picture style, so a fixed cutoff on one number picks every cell on one
//! page and none on the next. The comparison is made against alternatives instead:
//! the target phrase plus a fixed set of others, softmax over the set, and a cell
//! is chosen when the target wins by enough. That is the same shape as asking the
//! model to name the picture and checking whether it said the thing.

use serde::Deserialize;

/// One cell of the grid, in the crop's own pixels.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Cell {
    /// Left edge, relative to the crop.
    pub x: f64,
    /// Top edge, relative to the crop.
    pub y: f64,
    /// Cell width.
    pub w: f64,
    /// Cell height.
    pub h: f64,
}

/// Alternatives every cell is scored against.
///
/// A cell that holds the target should beat all of them; a cell that holds
/// something else should lose to at least one. They are deliberately generic: a
/// list of vendor objects would be a vendor list, and vendor knowledge belongs in
/// the catalog, not in the helper.
pub const ALTERNATIVES: [&str; 5] = [
    "a photo of an empty background",
    "a photo of a different object",
    "a photo of text",
    "a photo of a plain pattern",
    "a photo of scenery",
];

/// How much of the probability mass the target has to hold before a cell is
/// clicked.
///
/// Half is the point where the target is the single most likely reading of the
/// cell against every alternative at once. A grid answer is graded all-or-nothing
/// by the vendor, so a cell picked at 0.3 costs the whole solve.
pub const THRESHOLD: f32 = 0.5;

/// Scale between cosine similarity and logits, as the model was trained.
const LOGIT_SCALE: f32 = 100.0;

/// Lead-ins a widget puts in front of the thing it is asking for. Longest first,
/// so "select all squares with" is stripped before "select all".
const LEAD_INS: [&str; 12] = [
    "select all squares with",
    "select all images with",
    "select all images containing",
    "select each image containing",
    "click each image containing",
    "click on all images with",
    "select all the squares with",
    "please click each image containing",
    "click each",
    "select all",
    "click on all",
    "select each",
];

/// Words that carry no picture.
const LEADING_ARTICLES: [&str; 3] = ["a ", "an ", "the "];

/// The phrase to score a cell against, from the widget's own words.
///
/// The prompt is whatever the vendor wrote on the page. What the model wants is a
/// caption, so the instruction is removed and the object is put in the caption
/// form the weights were trained on.
#[must_use]
pub fn phrase(prompt: &str) -> String {
    let mut object = prompt.trim().trim_end_matches('.').to_lowercase();
    object = object.replace('\n', " ");
    while object.contains("  ") {
        object = object.replace("  ", " ");
    }
    for lead in LEAD_INS {
        if let Some(rest) = object.strip_prefix(lead) {
            object = rest.trim().to_string();
            break;
        }
    }
    for article in LEADING_ARTICLES {
        // A whole word, with or without anything after it: "the" on its own asks
        // for nothing, and "there" is not an article.
        let bare = article.trim_end();
        if object == bare {
            object = String::new();
            break;
        }
        if let Some(rest) = object.strip_prefix(article) {
            object = rest.to_string();
            break;
        }
    }
    let object = object.trim();
    if object.is_empty() {
        // Nothing was asked for. Scoring against "a photo of a" would pick cells
        // at random, so the caller is told the prompt carried no object.
        return String::new();
    }
    format!("a photo of a {object}")
}

/// Softmax over one cell's similarities, returning the target's share.
///
/// `similarities[0]` is the target; the rest are the alternatives.
#[must_use]
pub fn target_share(similarities: &[f32]) -> f32 {
    if similarities.is_empty() {
        return 0.0;
    }
    let logits: Vec<f32> = similarities.iter().map(|s| s * LOGIT_SCALE).collect();
    let top = logits.iter().copied().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = logits.iter().map(|l| (l - top).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum == 0.0 {
        return 0.0;
    }
    exps[0] / sum
}

/// Cells whose target share clears the threshold, in grid order.
#[must_use]
pub fn chosen(shares: &[f32], threshold: f32) -> Vec<usize> {
    shares
        .iter()
        .enumerate()
        .filter(|(_, share)| **share > threshold)
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_becomes_a_caption_of_what_it_asked_for() {
        for (prompt, want) in [
            ("Select all squares with a bicycle", "a photo of a bicycle"),
            ("Select all images with traffic lights.", "a photo of a traffic lights"),
            ("Click each image containing a bus", "a photo of a bus"),
            ("  select   all   squares  with  the  crosswalk ", "a photo of a crosswalk"),
            ("a red circle", "a photo of a red circle"),
        ] {
            assert_eq!(phrase(prompt), want, "prompt {prompt:?}");
        }
    }

    #[test]
    fn a_prompt_that_asks_for_nothing_yields_no_phrase() {
        // An empty caption would score every cell against the same words and pick
        // whichever the model liked, which is a random answer with a confidence.
        for prompt in ["", "   ", "select all", "Select all squares with", "the"] {
            assert_eq!(phrase(prompt), "", "prompt {prompt:?}");
        }
    }

    #[test]
    fn the_longest_lead_in_wins() {
        // "select all" is a prefix of "select all squares with". Stripping the
        // short one first leaves "squares with a bicycle" as the object.
        assert_eq!(phrase("Select all squares with a bicycle"), "a photo of a bicycle");
    }

    #[test]
    fn a_share_is_a_probability_over_the_alternatives() {
        let even = target_share(&[0.5, 0.5, 0.5]);
        assert!((even - 1.0 / 3.0).abs() < 1e-4, "{even}");
        let clear = target_share(&[0.5, 0.1, 0.1]);
        assert!(clear > 0.99, "{clear}");
        let lost = target_share(&[0.1, 0.5, 0.1]);
        assert!(lost < 0.01, "{lost}");
    }

    #[test]
    fn a_share_survives_a_similarity_that_is_the_same_everywhere() {
        // Equal similarities must not resolve to a confident pick through a
        // rounding artefact of the exponent.
        let share = target_share(&[0.2; 6]);
        assert!((share - 1.0 / 6.0).abs() < 1e-4, "{share}");
        assert!(chosen(&[share; 6], THRESHOLD).is_empty());
    }

    #[test]
    fn only_cells_over_the_threshold_are_chosen_and_they_keep_grid_order() {
        let shares = [0.9, 0.1, 0.51, 0.5, 0.99];
        assert_eq!(chosen(&shares, THRESHOLD), vec![0, 2, 4]);
    }

    #[test]
    fn a_grid_nothing_matches_chooses_nothing() {
        // A helper that always answers with a cell teaches its caller that an
        // answer means a match.
        assert!(chosen(&[0.2, 0.3, 0.1], THRESHOLD).is_empty());
    }
}
