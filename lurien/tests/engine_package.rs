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

/// The evidence schema version is one number held in two repositories.
///
/// The engine stamps rows, the driver refuses a stamp it does not know, and a
/// bump on one side alone turns every navigation into `Error::EvidenceVersion`
/// against a browser that is in fact current. Neither side can see the other at
/// run time, so the agreement is checked here.
#[test]
fn the_engine_stamps_the_version_the_driver_reads() {
    let dir = additions();
    if !dir.is_dir() {
        return;
    }
    let source = fs::read_to_string(dir.join("Observer.sys.mjs")).expect("Observer.sys.mjs");
    let needle = "export const EVIDENCE_VERSION = ";
    let index = source
        .find(needle)
        .expect("Observer.sys.mjs declares EVIDENCE_VERSION");
    let stamped: u64 = source[index + needle.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("EVIDENCE_VERSION is a number");
    assert_eq!(
        stamped,
        lurien::challenge::EVIDENCE_VERSION,
        "the engine stamps evidence schema {stamped} and the driver reads {}",
        lurien::challenge::EVIDENCE_VERSION
    );
}

/// A frozen array in the engine, read as a list of names.
fn js_string_list(source: &str, name: &str) -> Vec<String> {
    let needle = format!("export const {name} = Object.freeze([");
    let index = source
        .find(&needle)
        .unwrap_or_else(|| panic!("Kinds.sys.mjs declares {name}"));
    let rest = &source[index + needle.len()..];
    let end = rest.find("])").expect("the array closes");
    rest[..end]
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Reduction picks the kind that gates the page, so every kind needs a rank.
///
/// A kind with no rank sorts last, which is below `none`: adding a kind and
/// forgetting the severity table would make the new kind lose to a checkbox
/// beside it on the same page and never be acted on. That failure is invisible
/// on a page with one widget, so the table is held complete here instead.
#[test]
fn every_kind_has_a_place_in_the_order_that_gates_a_page() {
    let dir = additions();
    if !dir.is_dir() {
        return;
    }
    let source = fs::read_to_string(dir.join("Kinds.sys.mjs")).expect("Kinds.sys.mjs");
    let kinds = js_string_list(&source, "KINDS");
    let severity = js_string_list(&source, "KIND_SEVERITY");
    assert_eq!(
        kinds.iter().collect::<BTreeSet<_>>(),
        severity.iter().collect::<BTreeSet<_>>(),
        "KIND_SEVERITY and KINDS name different kinds: {kinds:?} against {severity:?}"
    );
    assert_eq!(
        severity.len(),
        kinds.len(),
        "KIND_SEVERITY names a kind twice: {severity:?}"
    );
    assert_eq!(
        severity.first().map(String::as_str),
        Some("fail"),
        "a page that already failed is the most severe thing on it: {severity:?}"
    );
    assert_eq!(
        severity.last().map(String::as_str),
        Some("none"),
        "no challenge must rank below every challenge: {severity:?}"
    );
    // An interactive kind outranks a passive one: a score resolves itself while a
    // widget stays in the way, so a page holding both is a widget page.
    let rank = |kind: &str| severity.iter().position(|item| item == kind);
    for interactive in ["visual", "audio", "slider", "pow", "checkbox"] {
        assert!(
            rank(interactive) < rank("score"),
            "{interactive} does not outrank score: {severity:?}"
        );
    }
}
