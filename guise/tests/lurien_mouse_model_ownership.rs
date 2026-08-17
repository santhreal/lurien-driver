//! Cross-project coherence audit: guise owns the mouse model; lurien must not
//! duplicate it (G131/R108/R266).
//!
//! The BiDi path drives mouse motion via `input.performActions` with trajectories
//! computed in `guise::human::mouse`, so lurien's native `MouseTrajectories.hpp`
//! / `camouGetMouseTrajectory` dead code was removed. This test is a regression
//! fence: if any of those symbols reappear in the lurien source tree, the build
//! fails loudly so the duplication cannot silently return.

use std::fs;
use std::path::{Path, PathBuf};

fn lurien_root() -> PathBuf {
    let guise_crate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // guise is at software/browser/guise; engine is the sibling engine/.
    guise_crate.join("../engine")
}

fn is_source_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    matches!(
        ext,
        "cpp"
            | "h"
            | "hpp"
            | "patch"
            | "webidl"
            | "py"
            | "js"
            | "mjs"
            | "sys.mjs"
            | "rs"
            | "toml"
            | "build"
    )
}

fn walk_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip symlinks to the out-of-tree build tree; the source of truth is
        // the patches/ and additions/ directories in the lurien repo.
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            // Avoid descending into build artifacts / object trees if they exist
            // in the source tree.
            if path.file_name().is_some_and(|n| {
                n == "obj-x86_64-pc-linux-gnu"
                    || n == "node_modules"
                    || n == ".git"
                    || n == "camoufox-150.0.2-beta.25"
            }) {
                continue;
            }
            walk_files(&path, files);
        } else if path.is_file() && is_source_file(&path) {
            files.push(path);
        }
    }
}

#[test]
fn lurien_mouse_trajectories_header_is_removed() {
    let header = lurien_root().join("additions/camoucfg/MouseTrajectories.hpp");
    assert!(
        !header.exists(),
        "MouseTrajectories.hpp must be removed from lurien; guise owns the mouse model"
    );
}

#[test]
fn lurien_source_contains_no_mouse_trajectory_symbols() {
    let root = lurien_root();
    assert!(root.exists(), "lurien source root must exist: {root:?}");

    let forbidden = [
        "HumanizeMouseTrajectory",
        "MouseTrajectories.hpp",
        "camouGetMouseTrajectory",
        "CamouGetMouseTrajectory",
    ];
    let mut files = Vec::new();
    walk_files(&root, &mut files);

    let mut violations = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for symbol in &forbidden {
            if text.contains(symbol) {
                violations.push(format!("{symbol} found in {}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "lurien source must not contain mouse-trajectory duplication; guise owns the model:\n{}",
        violations.join("\n")
    );
}
