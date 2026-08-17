//! Laws about the engine additions as a *package*.
//!
//! A module in `engine/additions/challenge/` is only real once the jar manifest
//! carries it: an unlisted file is absent from the built browser, and the first
//! `resource://lurien-challenge/` import of it fails inside a content process
//! where nothing in this repository can see the error. The same goes for an
//! import that names a file which does not exist. Both are run-time-only
//! failures otherwise, so both are checked here from the directory itself rather
//! than from a list somebody has to remember to update.
//!
//! The engine is a sibling repository and `engine/` is not checked out here, so
//! these laws pass vacuously where the directory is missing. That is the point:
//! a module is only ever added beside a checkout that has it, which is where the
//! check has to be red.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn additions() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/additions/challenge")
}

/// Every module file in the directory, by file name.
fn module_files(dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(dir).expect("challenge additions directory") {
        let path = entry.expect("entry").path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if name.ends_with(".sys.mjs") || name.ends_with(".js") {
            names.insert(name);
        }
    }
    names
}

#[test]
fn every_engine_module_is_in_the_jar_manifest() {
    let dir = additions();
    if !dir.is_dir() {
        return;
    }
    let manifest = fs::read_to_string(dir.join("jar.mn")).expect("jar.mn");
    for name in module_files(&dir) {
        assert!(
            manifest.contains(&format!("content/{name} ({name})")),
            "{name} exists but jar.mn does not package it, so the built browser has no \
             resource://lurien-challenge/{name}"
        );
    }
}

#[test]
fn every_manifest_entry_names_a_file_that_exists() {
    let dir = additions();
    if !dir.is_dir() {
        return;
    }
    let manifest = fs::read_to_string(dir.join("jar.mn")).expect("jar.mn");
    for line in manifest.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("content/") else {
            continue;
        };
        let source = rest
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .unwrap_or_default();
        assert!(
            !source.is_empty(),
            "jar.mn line {line:?} packages nothing readable"
        );
        assert!(
            dir.join(source).is_file(),
            "jar.mn packages {source}, which does not exist in {}",
            dir.display()
        );
    }
}

#[test]
fn every_module_import_resolves_to_a_packaged_module() {
    let dir = additions();
    if !dir.is_dir() {
        return;
    }
    let present = module_files(&dir);
    for name in &present {
        let source = fs::read_to_string(dir.join(name)).unwrap_or_default();
        for (index, _) in source.match_indices("resource://lurien-challenge/") {
            let tail = &source[index + "resource://lurien-challenge/".len()..];
            let imported: String = tail
                .chars()
                .take_while(|c| !matches!(c, '"' | '\'' | '`' | ' ' | ')' | '\n'))
                .collect();
            // A directory substitution (the source-run path) names no file.
            if imported.is_empty() {
                continue;
            }
            assert!(
                present.contains(&imported),
                "{name} imports resource://lurien-challenge/{imported}, which is not a module in {}",
                dir.display()
            );
        }
    }
}
