//! The grid: which cells answer the question the widget asked.
//!
//! The browser owns geometry. It knows where the cells are, because it laid them
//! out, and it sends their rectangles with the crop. This file decides which of
//! them match the phrase, and nothing here knows what a browsing context is.
//!
//! A detector answers per box, not per tile, so the decision is an attribution and
//! a cut. A box belongs to the cell its centre falls in, a cell's score is the
//! strongest box attributed to it, and a cell is chosen when that score clears
//! both an absolute floor and a share of the strongest box anywhere in the crop.
//! The floor is what keeps a grid holding nothing the widget asked for from being
//! answered at all; the share is what keeps one drawing style's confidence from
//! becoming a different cutoff on the next page.

use serde::Deserialize;

use crate::detect::Detection;

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

impl Cell {
    /// The same cell in the picture's pixels, when the snapshot was taken at a
    /// scale the caller did not state its rectangles in.
    #[must_use]
    pub fn scaled(&self, scale: f64) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
            w: self.w * scale,
            h: self.h * scale,
        }
    }

    /// Whether a point is inside the cell. The right and bottom edges belong to
    /// the next cell, so a box centred exactly on a shared edge is attributed
    /// once.
    #[must_use]
    pub fn holds(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// The score a cell has to reach whatever else is in the crop.
///
/// A detector's confidence in an object that is not there is small but not zero:
/// measured on a fixture grid of drawn shapes, a query for an absent shape peaked
/// at 0.052 across nine tiles, while the shape that was there scored 0.408 and up.
/// On a live vendor grid the true tiles scored 0.174 and up with nothing else over
/// 0.041. The floor sits above the noise of both.
pub const ABS_MIN: f32 = 0.08;

/// The share of the strongest box in the crop a cell has to hold.
///
/// Confidence is not comparable between pages: a flat drawn shape scores twice
/// what a small photographed object does. What is comparable inside one crop is
/// the ratio, because every tile was drawn by the same widget in the same style.
///
/// Measured over 24 fixture grids and two other grids, once tile-sized boxes are
/// out of the way, the weakest true cell held 0.75 of the strongest and the
/// loudest wrong cell 0.29 of the weakest true one. The cut sits between them.
pub const RELATIVE: f32 = 0.4;

/// The share of a cell a box has to cover before it is the cell rather than
/// something in it.
///
/// A tile is a rectangle, so a detector asked for a square finds one in every
/// tile: measured on the fixture grid, the box around an empty tile scored 0.194
/// for "a photo of a blue square" against 0.417 for a tile that held one, which is
/// close enough to answer the wrong set. Those boxes are the widget's own
/// furniture. They are dropped when the crop holds anything smaller, and kept when
/// it does not, because on a live grid the object can fill its tile.
pub const CELL_BOX: f64 = 0.85;

/// The weakest box worth writing down.
///
/// Nothing under `ABS_MIN` can be chosen, so this changes no answer. It is what
/// keeps a refusal readable: a grid the detector nearly answered and a grid it saw
/// nothing in both report zeros otherwise, and the difference is the whole
/// diagnosis.
pub const REPORT_MIN: f32 = 0.02;

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

/// The phrase to look for, from the widget's own words.
///
/// The prompt is whatever the vendor wrote on the page. What the model wants is a
/// caption naming one object, so the instruction is removed and the object is put
/// in the caption form the weights were trained on.
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
        // Nothing was asked for. Looking for "a photo of a" would find whatever
        // the detector liked, which is a random answer with a confidence.
        return String::new();
    }
    format!("a photo of a {object}")
}

/// The strongest box attributed to each cell, in grid order.
///
/// A box whose centre is in no cell is dropped rather than given to the nearest
/// one: the gutter between tiles is where a widget draws its own furniture, and a
/// detection there is not evidence about either neighbour.
///
/// Boxes that fill their cell are read second. They are what a detector returns
/// when the phrase describes the tile as well as its contents, so as long as the
/// crop holds one smaller box worth reporting, the tiles themselves are ignored. A
/// grid whose objects fill their tiles has nothing smaller, and then the tile-sized
/// boxes are the answer.
#[must_use]
pub fn cell_scores(cells: &[Cell], detections: &[Detection]) -> Vec<f32> {
    let inner = attributed(cells, detections, true);
    if inner.iter().copied().fold(0.0f32, f32::max) >= ABS_MIN {
        return inner;
    }
    attributed(cells, detections, false)
}

/// The strongest box in each cell, with or without the boxes that are the cell.
fn attributed(cells: &[Cell], detections: &[Detection], inner_only: bool) -> Vec<f32> {
    let mut scores = vec![0.0f32; cells.len()];
    for found in detections {
        let Some(index) = cells.iter().position(|cell| cell.holds(found.cx, found.cy)) else {
            continue;
        };
        let cell = cells[index];
        if inner_only && found.w * found.h >= CELL_BOX * cell.w * cell.h {
            continue;
        }
        if found.score > scores[index] {
            scores[index] = found.score;
        }
    }
    scores
}

/// The score a cell has to reach in this crop, from the strongest cell in it.
///
/// An empty crop has no strongest cell, so the cut is the floor and nothing
/// clears it.
#[must_use]
pub fn cut(scores: &[f32]) -> f32 {
    let best = scores.iter().copied().fold(0.0f32, f32::max);
    ABS_MIN.max(best * RELATIVE)
}

/// Cells that answer the question, in grid order.
#[must_use]
pub fn chosen(scores: &[f32]) -> Vec<usize> {
    let cut = cut(scores);
    scores
        .iter()
        .enumerate()
        .filter(|(_, score)| **score >= cut)
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A detection at a point, strong enough to matter.
    fn at(score: f32, cx: f64, cy: f64) -> Detection {
        Detection {
            score,
            query: 0,
            cx,
            cy,
            w: 10.0,
            h: 10.0,
        }
    }

    /// Three cells side by side, 100 wide, no gutter.
    fn row() -> Vec<Cell> {
        (0..3)
            .map(|i| Cell {
                x: f64::from(i) * 100.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .collect()
    }

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
        // An empty caption would look for the same nothing in every cell and pick
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
    fn a_cell_takes_the_strongest_box_whose_centre_is_inside_it() {
        let scores = cell_scores(
            &row(),
            &[at(0.2, 50.0, 50.0), at(0.5, 60.0, 20.0), at(0.3, 150.0, 50.0)],
        );
        assert_eq!(scores, vec![0.5, 0.3, 0.0]);
    }

    #[test]
    fn a_box_in_the_gutter_is_not_attributed_to_a_neighbour() {
        // Cells with a gap between them: a detection in the gap is the widget's
        // own furniture, not evidence about either side.
        let cells: Vec<Cell> = (0..2)
            .map(|i| Cell {
                x: f64::from(i) * 120.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .collect();
        assert_eq!(cell_scores(&cells, &[at(0.9, 110.0, 50.0)]), vec![0.0, 0.0]);
        // And a box centred on the shared edge of two touching cells lands in one.
        assert_eq!(cell_scores(&row(), &[at(0.9, 100.0, 0.0)]), vec![0.0, 0.9, 0.0]);
    }

    #[test]
    fn a_box_that_is_the_cell_loses_to_anything_smaller_in_the_crop() {
        // The frame of an empty tile is a square, a rectangle, a border: a detector
        // asked for one finds it in every tile. Those boxes are read only when the
        // crop holds nothing smaller.
        let cells = row();
        let tile = |score: f32, cx: f64| Detection {
            score,
            query: 0,
            cx,
            cy: 50.0,
            w: 100.0,
            h: 100.0,
        };
        let scores = cell_scores(&cells, &[tile(0.19, 50.0), tile(0.18, 150.0), at(0.26, 250.0, 50.0)]);
        assert_eq!(scores, vec![0.0, 0.0, 0.26]);
        assert_eq!(chosen(&scores), vec![2]);
        // Nothing smaller anywhere: the tiles are all there is to go on, and a grid
        // whose objects fill their tiles still gets an answer.
        let filled = cell_scores(&cells, &[tile(0.6, 50.0), tile(0.55, 150.0), tile(0.05, 250.0)]);
        assert_eq!(filled, vec![0.6, 0.55, 0.05]);
        assert_eq!(chosen(&filled), vec![0, 1]);
    }

    #[test]
    fn only_cells_over_the_cut_are_chosen_and_they_keep_grid_order() {
        // The strongest cell is 0.5, so the cut is 0.2: 0.19 misses it.
        let scores = [0.5, 0.19, 0.2, 0.0, 0.45];
        assert!((cut(&scores) - 0.2).abs() < 1e-6, "{}", cut(&scores));
        assert_eq!(chosen(&scores), vec![0, 2, 4]);
    }

    #[test]
    fn a_grid_nothing_matches_chooses_nothing() {
        // A helper that always answers with a cell teaches its caller that an
        // answer means a match. The floor is what makes an empty grid an empty
        // answer: relative alone would pick the loudest noise.
        assert!(chosen(&[0.05, 0.04, 0.02]).is_empty());
        assert!(chosen(&[0.0; 9]).is_empty());
        assert!(chosen(&[]).is_empty());
    }

    #[test]
    fn every_measured_grid_answers_its_own_tiles_and_no_others() {
        // Cell scores recorded from five grids, in tile order, after tile-sized
        // boxes were dropped. Each is a set a vendor or a fixture graded, so a cut
        // that admits one tile more or one fewer costs the solve.
        for (name, scores, want) in [
            (
                "live vendor cars",
                vec![0.0, 0.0, 0.22, 0.028, 0.209, 0.03, 0.027, 0.174, 0.041],
                vec![2, 4, 7],
            ),
            (
                "450px drawn shapes",
                vec![0.048, 0.424, 0.076, 0.445, 0.034, 0.066, 0.05, 0.427, 0.043],
                vec![1, 3, 7],
            ),
            (
                "336px fixture, two of nine",
                vec![0.026, 0.02, 0.024, 0.025, 0.26, 0.304, 0.029, 0.024, 0.316],
                vec![4, 5, 8],
            ),
            (
                "336px fixture, four of nine",
                vec![0.307, 0.326, 0.038, 0.031, 0.323, 0.395, 0.082, 0.033, 0.088],
                vec![0, 1, 4, 5],
            ),
            (
                "336px fixture, a triangle",
                vec![0.031, 0.034, 0.611, 0.628, 0.025, 0.033, 0.072, 0.068, 0.479],
                vec![2, 3, 8],
            ),
        ] {
            assert_eq!(chosen(&scores), want, "{name}: cut {}", cut(&scores));
        }
    }
}
