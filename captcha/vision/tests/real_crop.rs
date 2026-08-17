//! The measurement, against a crop the browser actually produced.
//!
//! The unit tests in `gap.rs` build their own images, so they only ever contain
//! the shapes the author thought of. This crop came off `drawSnapshot` on the
//! slider fixture: a bordered widget, a gradient background, a piece parked at
//! x=10 and a notch minted at x=134, all rendered by Gecko rather than by a test.
//!
//! The travel it pins, 124 px, is the fixture's own arithmetic: notch column
//! minus piece home. A change to the finder that reads the widget border as the
//! piece scores 43 here, which is the defect this file exists to catch.

use lurien_vision::gap::{find, Gray};

/// Piece home and notch column of the recorded crop, in crop pixels.
const PIECE_X: usize = 10;
const GAP_X: usize = 134;

fn recorded() -> Gray {
    let bytes = include_bytes!("crops/slider_fixture_322x142.png");
    lurien_vision::decode_png(bytes).expect("the recorded crop decodes")
}

#[test]
fn the_recorded_crop_measures_its_own_travel() {
    let image = recorded();
    assert_eq!((image.width, image.height), (322, 142));
    let found = find(&image).expect("the recorded crop has a piece and a notch");
    assert!(
        found.piece_x.abs_diff(PIECE_X) <= 2,
        "the piece was found at {} rather than {PIECE_X}: the widget border was read as a shape",
        found.piece_x
    );
    assert!(
        found.gap_x.abs_diff(GAP_X) <= 2,
        "the notch was found at {} rather than {GAP_X}",
        found.gap_x
    );
    assert!(
        found.dx.abs_diff(GAP_X - PIECE_X) <= 3,
        "the travel came out as {} rather than {}",
        found.dx,
        GAP_X - PIECE_X
    );
    assert!(
        found.confidence >= 100.0,
        "a rendered notch on a gradient scored only {}",
        found.confidence
    );
}
