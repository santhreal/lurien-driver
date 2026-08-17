//! The detector, against grids that hold known objects in known cells.
//!
//! The unit tests in `grid.rs` feed the decision rule numbers, so they prove the
//! attribution and the cut and nothing about perception. This file draws nine cell
//! grids of shapes, asks for one of them, and checks that the cells it gets back are
//! the cells that hold it. That is the whole product claim for a visual kind: the
//! answer is a set of indices, and a wrong set costs the solve.
//!
//! Two geometries are drawn. One is 150 px cells edge to edge, which is a grid at a
//! comfortable size. The other is the geometry the `visual` fixture paints and the
//! engine snapshots: 108 px tiles with a one pixel border and a six pixel gutter,
//! shapes on a 106 px canvas inside. The second one is the hard one, because a
//! bordered empty tile is itself a square, and that is what the tile-box rule in
//! `grid.rs` exists for.
//!
//! The weights are not in this repository. Without them the file says so and
//! passes: a checkout that cannot see is a checkout that refuses grids, which is
//! the behaviour `lib.rs` already tests. It is skipped loudly rather than silently
//! so a run that proves nothing does not read like a run that proved something.

use std::path::PathBuf;

use lurien_vision::detect::Detector;
use lurien_vision::grid::{cell_scores, chosen, phrase, Cell, REPORT_MIN};
use lurien_vision::pixels::Rgb;

/// Cells per side.
const SIDE: usize = 3;

/// The shapes each cell holds, in grid order.
const LAYOUT: [Shape; 9] = [
    Shape::Circle,
    Shape::Square,
    Shape::Triangle,
    Shape::Square,
    Shape::Circle,
    Shape::Triangle,
    Shape::Triangle,
    Shape::Square,
    Shape::Circle,
];

/// A grid's geometry, in the pixels a snapshot would carry.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// Tile pitch, border included.
    tile: usize,
    /// White space between tiles.
    gutter: usize,
    /// Whether each tile is outlined, the way the fixture outlines its own.
    border: bool,
}

/// Cells edge to edge, no furniture.
const PLAIN: Geometry = Geometry {
    tile: 150,
    gutter: 0,
    border: false,
};

/// What the `visual` fixture paints: `grid-template-columns: repeat(3, 108px)`,
/// `gap: 6px`, a 1px `#ccc` border per tile, a 106px canvas inside it.
const FIXTURE: Geometry = Geometry {
    tile: 108,
    gutter: 6,
    border: true,
};

impl Geometry {
    /// Side of the whole grid.
    fn side(self) -> usize {
        SIDE * self.tile + (SIDE - 1) * self.gutter
    }

    /// Side of the canvas a shape is drawn on, inside one tile.
    fn canvas(self) -> usize {
        if self.border {
            self.tile - 2
        } else {
            self.tile
        }
    }

    /// Top left of a tile's canvas.
    fn origin(self, index: usize) -> (usize, usize) {
        let inset = usize::from(self.border);
        (
            (index % SIDE) * (self.tile + self.gutter) + inset,
            (index / SIDE) * (self.tile + self.gutter) + inset,
        )
    }

    /// The cell rectangles a browser would report, tile boxes and not canvases.
    fn cells(self) -> Vec<Cell> {
        (0..LAYOUT.len())
            .map(|index| Cell {
                x: ((index % SIDE) * (self.tile + self.gutter)) as f64,
                y: ((index / SIDE) * (self.tile + self.gutter)) as f64,
                w: self.tile as f64,
                h: self.tile as f64,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A filled red disc.
    Circle,
    /// A filled blue square.
    Square,
    /// A filled green triangle, apex up.
    Triangle,
}

impl Shape {
    /// The words the fixture writes for it.
    fn words(self) -> &'static str {
        match self {
            Shape::Circle => "red circle",
            Shape::Square => "blue square",
            Shape::Triangle => "green triangle",
        }
    }

    /// The fixture's own colours.
    fn colour(self) -> [u8; 3] {
        match self {
            Shape::Circle => [0xd0, 0x20, 0x2a],
            Shape::Square => [0x1b, 0x45, 0xc8],
            Shape::Triangle => [0x1c, 0x8a, 0x3c],
        }
    }

    /// Whether a point on a `side` by `side` canvas is painted. The fractions are
    /// the fixture's: radius 0.36, square 0.62, triangle height 0.66 and base 1.2
    /// times that height.
    fn covers(self, side: f64, x: f64, y: f64) -> bool {
        let mid = side / 2.0;
        match self {
            Shape::Circle => (x - mid).hypot(y - mid) <= side * 0.36,
            Shape::Square => {
                let half = side * 0.31;
                (x - mid).abs() <= half && (y - mid).abs() <= half
            }
            Shape::Triangle => {
                let height = side * 0.66;
                let top = (side - height) / 2.0;
                let base = (side + height) / 2.0;
                if y < top || y > base {
                    return false;
                }
                let half = (y - top) / height * height * 0.6;
                (x - mid).abs() <= half
            }
        }
    }
}

/// The grid, drawn white with one filled shape per tile.
///
/// Edges are supersampled: a hard-edged disc is a polygon of stair steps, and a
/// detector asked for a circle is entitled to be shown one.
fn drawn(geometry: Geometry) -> Rgb {
    let side = geometry.side();
    let canvas = geometry.canvas() as f64;
    let mut data = vec![255u8; side * side * 3];
    let mut put = |x: usize, y: usize, colour: [u8; 3], coverage: f64| {
        let at = (y * side + x) * 3;
        for channel in 0..3 {
            let paint = f64::from(colour[channel]);
            let blend = paint * coverage + 255.0 * (1.0 - coverage);
            data[at + channel] = blend.round() as u8;
        }
    };
    for (index, shape) in LAYOUT.iter().enumerate() {
        let (left, top) = geometry.origin(index);
        if geometry.border {
            // The tile's own outline, which is the square a detector asked for a
            // square keeps finding.
            let box_left = left - 1;
            let box_top = top - 1;
            for step in 0..geometry.tile {
                put(box_left + step, box_top, [0xcc, 0xcc, 0xcc], 1.0);
                put(box_left + step, box_top + geometry.tile - 1, [0xcc, 0xcc, 0xcc], 1.0);
                put(box_left, box_top + step, [0xcc, 0xcc, 0xcc], 1.0);
                put(box_left + geometry.tile - 1, box_top + step, [0xcc, 0xcc, 0xcc], 1.0);
            }
        }
        let colour = shape.colour();
        for y in 0..geometry.canvas() {
            for x in 0..geometry.canvas() {
                let mut inside = 0u32;
                for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    if shape.covers(canvas, x as f64 + dx, y as f64 + dy) {
                        inside += 1;
                    }
                }
                if inside == 0 {
                    continue;
                }
                put(left + x, top + y, colour, f64::from(inside) / 4.0);
            }
        }
    }
    Rgb::new(side, side, data)
}

/// The model directory, if this host has one.
fn model_dir() -> Option<PathBuf> {
    if let Some(dir) = lurien_vision::detect::model_dir_from_env() {
        return dir.is_dir().then_some(dir);
    }
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".cache/lurien/vision/owlvit-base-patch32");
    dir.join("model.onnx").is_file().then_some(dir)
}

/// A loaded detector, or a reason this host cannot prove anything.
fn detector() -> Option<Detector> {
    let dir = model_dir().or_else(|| {
        eprintln!(
            "SKIP: no detector export in $LURIEN_VISION_MODEL or \
             ~/.cache/lurien/vision/owlvit-base-patch32; grid perception is unproven here"
        );
        None
    })?;
    match Detector::load(&dir) {
        Ok(detector) => Some(detector),
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// The cells a grid's own question is answered with.
fn answer(detector: &mut Detector, geometry: Geometry, prompt: &str) -> (Vec<usize>, Vec<f32>) {
    let image = drawn(geometry);
    let asked = phrase(prompt);
    let found = detector
        .detect(&image, &[asked], REPORT_MIN)
        .expect("the detector answers a grid it can see");
    let scores = cell_scores(&geometry.cells(), &found);
    (chosen(&scores), scores)
}

#[test]
fn every_shape_is_found_in_the_cells_that_hold_it_and_nowhere_else() {
    let Some(mut detector) = detector() else { return };
    for geometry in [PLAIN, FIXTURE] {
        for shape in [Shape::Circle, Shape::Square, Shape::Triangle] {
            let want: Vec<usize> = LAYOUT
                .iter()
                .enumerate()
                .filter(|(_, held)| **held == shape)
                .map(|(index, _)| index)
                .collect();
            let prompt = format!("Select all images with a {}", shape.words());
            let (got, scores) = answer(&mut detector, geometry, &prompt);
            assert_eq!(
                got, want,
                "{prompt:?} on a {} px grid answered the wrong cells; scores {scores:?}",
                geometry.side()
            );
        }
    }
}

#[test]
fn a_shape_the_grid_does_not_hold_is_answered_with_no_cells() {
    // The floor is what makes this an empty answer rather than the loudest noise
    // in the crop: without it, every grid has a strongest cell and every strongest
    // cell clears a purely relative cut.
    let Some(mut detector) = detector() else { return };
    for geometry in [PLAIN, FIXTURE] {
        for absent in ["a yellow star", "a bicycle", "a traffic light"] {
            let prompt = format!("Select all images with {absent}");
            let (got, scores) = answer(&mut detector, geometry, &prompt);
            assert!(
                got.is_empty(),
                "{prompt:?} on a {} px grid was answered with cells {got:?}; scores {scores:?}",
                geometry.side()
            );
        }
    }
}
