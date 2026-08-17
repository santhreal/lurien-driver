//! Closed kinds + vendor TOML. A new kind without a schema row is red.
//! A vendor that names an unknown kind is red.

use std::fs;
use std::path::PathBuf;

fn kinds_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("kinds")
}

fn closed_kinds() -> Vec<String> {
    let raw = fs::read_to_string(kinds_dir().join("_schema.toml")).expect("_schema.toml");
    let mut kinds = Vec::new();
    let mut in_list = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("kinds") && t.contains('[') {
            in_list = true;
        }
        if in_list {
            if let Some(name) = t.trim_matches(&[',', '"', ' '][..]).strip_prefix('"') {
                let _ = name;
            }
            if t.starts_with('"') {
                kinds.push(t.trim_matches(&[',', '"'][..]).to_string());
            }
            if t.contains(']') {
                break;
            }
        }
    }
    kinds
}

#[test]
fn schema_lists_the_closed_set() {
    let kinds = closed_kinds();
    assert_eq!(
        kinds,
        ["none", "score", "checkbox", "visual", "slider", "audio", "pow", "fail"]
    );
}

#[test]
fn every_vendor_toml_names_a_closed_kind() {
    let kinds = closed_kinds();
    let dir = kinds_dir();
    let mut vendors = 0;
    for entry in fs::read_dir(&dir).expect("kinds dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if path.file_name().and_then(|s| s.to_str()) == Some("_schema.toml") {
            continue;
        }
        vendors += 1;
        let raw = fs::read_to_string(&path).expect("vendor toml");
        let kind = raw
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("kind")
                    .and_then(|rest| rest.trim().strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
            .unwrap_or_default();
        assert!(
            kinds.iter().any(|k| k == &kind),
            "{} names unknown kind {kind:?}",
            path.display()
        );
    }
    assert!(vendors >= 1, "at least one vendor TOML is required");
}

#[test]
fn every_closed_kind_has_a_fixture() {
    let dir = kinds_dir().join("fixtures");
    for kind in closed_kinds() {
        let path = dir.join(format!("{kind}.html"));
        assert!(path.is_file(), "missing fixture {}", path.display());
    }
    assert!(dir.join("cloudflare_score.html").is_file());
}

/// Required field names from `_schema.toml`, so the list is read at run time and
/// a field added to the schema turns this suite red until every vendor has it.
fn required_fields() -> Vec<String> {
    let raw = fs::read_to_string(kinds_dir().join("_schema.toml")).expect("_schema.toml");
    let mut fields = Vec::new();
    let mut in_list = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("vendor_fields") && t.contains('[') {
            in_list = true;
            continue;
        }
        if in_list {
            if t.contains(']') {
                break;
            }
            if t.starts_with('"') {
                fields.push(t.trim_matches(&[',', '"'][..]).to_string());
            }
        }
    }
    assert!(!fields.is_empty(), "schema lists no vendor_fields");
    fields
}

fn vendor_files() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(kinds_dir())
        .expect("kinds dir")
        .filter_map(|e| {
            let path = e.expect("entry").path();
            let is_toml = path.extension().and_then(|s| s.to_str()) == Some("toml");
            let is_schema = path.file_name().and_then(|s| s.to_str()) == Some("_schema.toml");
            (is_toml && !is_schema).then_some(path)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn every_vendor_binding_carries_every_required_field() {
    // A binding missing `token` or `target` is a vendor the engine cannot act on,
    // and the failure would otherwise surface as a silent no-op on a live page.
    for path in vendor_files() {
        let raw = fs::read_to_string(&path).expect("vendor toml");
        for field in required_fields() {
            let key = field.rsplit('.').next().expect("field name");
            assert!(
                raw.contains(key),
                "{} is missing required field {field}",
                path.display()
            );
        }
    }
}

#[test]
fn every_interactive_kind_has_at_least_one_vendor_binding() {
    // `none` and `fail` are outcomes, not vendors. Every other closed kind must
    // name a real vendor, or the kind is a label with nothing behind it.
    let bound: Vec<String> = vendor_files()
        .iter()
        .filter_map(|path| {
            let raw = fs::read_to_string(path).ok()?;
            raw.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("kind")
                    .and_then(|rest| rest.trim().strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        })
        .collect();
    for kind in closed_kinds() {
        if kind == "none" || kind == "fail" {
            continue;
        }
        assert!(
            bound.contains(&kind),
            "closed kind {kind} has no vendor TOML binding it"
        );
    }
}

#[test]
fn no_vendor_binding_leaks_a_vendor_name_into_the_engine() {
    // Vendors are data. If a name reaches the C++ additions, the engine has
    // learned a vendor and the catalog stopped being the single owner.
    let additions = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/additions/challenge");
    if !additions.is_dir() {
        return;
    }
    let names: Vec<String> = vendor_files()
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    for entry in fs::read_dir(&additions).expect("challenge dir") {
        let path = entry.expect("entry").path();
        if !path.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&path).unwrap_or_default().to_lowercase();
        for name in &names {
            assert!(
                !raw.contains(name),
                "{} names vendor {name}; vendors live in captcha/kinds/",
                path.display()
            );
        }
    }
}

/// The engine acts on a kind only when a dated scorecard row says a run proved
/// it. Without this, "claimed" would mean "somebody edited a constant".
#[test]
fn every_claimed_kind_has_a_dated_scorecard_row() {
    let scorecard = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/bench-results/challenge-scorecard.md");
    let raw = fs::read_to_string(&scorecard).expect("challenge-scorecard.md");
    for kind in lurien::challenge::CLAIMED_KINDS {
        let needle = format!("| `{kind}` |");
        assert!(
            raw.contains(&needle),
            "kind {kind} is claimed by the engine with no row in {}",
            scorecard.display()
        );
    }
    for row in raw.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let kind = cells.get(1).copied().unwrap_or_default().trim_matches('`');
        if !lurien::challenge::CLAIMED_KINDS.contains(&kind) {
            continue;
        }
        let date = cells.get(3).copied().unwrap_or_default();
        assert!(
            date == "n/a" || date.len() == 10 && date.starts_with("202"),
            "the scorecard row for {kind} has no date: {row}"
        );
    }
}

/// Every closed kind is either claimed with a row, or listed as not claimed. A
/// kind that is neither would fall through both doors in silence.
#[test]
fn every_closed_kind_is_either_claimed_or_named_as_refused() {
    let scorecard = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/bench-results/challenge-scorecard.md");
    let raw = fs::read_to_string(&scorecard).expect("challenge-scorecard.md");
    let (_, refused) = raw
        .split_once("## Not claimed")
        .expect("the scorecard names the kinds it refuses");
    for kind in closed_kinds() {
        if lurien::challenge::CLAIMED_KINDS.contains(&kind.as_str()) {
            continue;
        }
        assert!(
            refused.contains(&format!("`{kind}`")),
            "kind {kind} is neither claimed nor named as refused in the scorecard"
        );
    }
}
