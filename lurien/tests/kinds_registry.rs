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

/// A claim is only worth what someone can run again.
///
/// Every claimed kind's row must name a script in `lurien/tests/`, and that script
/// must exist. A `live vendor` row is exempt: its proof is a deployment, which is
/// not in the tree and cannot be replayed here. So a kind whose only proof was a
/// page somebody visited once must gain a fixture row before it may be claimed, and
/// a script renamed out from under a row turns this red instead of leaving the
/// claim pointing at nothing.
#[test]
fn every_claimed_kind_names_a_proof_that_can_be_run_again() {
    let scorecard = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/bench-results/challenge-scorecard.md");
    let raw = fs::read_to_string(&scorecard).expect("challenge-scorecard.md");
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut fixtures = 0;
    for row in raw.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let kind = cells.get(1).copied().unwrap_or_default().trim_matches('`');
        if !lurien::challenge::CLAIMED_KINDS.contains(&kind) {
            continue;
        }
        let class = cells.get(2).copied().unwrap_or_default();
        if class != "fixture" {
            continue;
        }
        fixtures += 1;
        let evidence = cells.get(7).copied().unwrap_or_default();
        let script = evidence
            .split_whitespace()
            .map(|word| word.trim_matches(|c: char| !c.is_ascii_graphic() || "`;,".contains(c)))
            .find(|word| word.ends_with(".sh"))
            .unwrap_or_else(|| panic!("the {kind} row names no script to rerun: {row}"));
        let name = script.rsplit('/').next().unwrap_or(script);
        assert!(
            tests.join(name).is_file(),
            "the {kind} row names {script}, which is not a file in {}",
            tests.display()
        );
    }
    assert!(
        fixtures >= 4,
        "only {fixtures} claimed kinds are proven by a script in this tree"
    );
}

/// The browser version `engine/upstream.sh` pins, as the scorecard writes it.
///
/// Vacuous where the engine is not checked out beside the driver, exactly as the
/// packaging laws are: a proof is only ever produced next to a checkout that has
/// it, which is where this has to be red.
fn pinned_engine() -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/upstream.sh");
    let raw = fs::read_to_string(path).ok()?;
    let mut version = None;
    let mut release = None;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            version = Some(value.trim().to_string());
        }
        if let Some(value) = line.strip_prefix("release=") {
            release = Some(value.trim().to_string());
        }
    }
    Some(format!("{}-{}", version?, release?))
}

/// A version's `major.minor`, which is the axis a `0.x` crate breaks on.
fn minor_of(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// A proof belongs to a build, not to a feature.
///
/// A dated row says somebody ran something once. It does not say the run happened
/// against the browser this tree builds or the driver that ships with it, and a
/// solver's proof is worth exactly the build it was produced on: the engine's
/// trusted-input path, its actor lifetimes and its token observation all move with
/// the browser version, and the config contract moves with the driver. So each row
/// names both, and a claim whose row names an older build is red here until the run
/// is repeated.
#[test]
fn every_claim_names_the_build_that_proved_it() {
    let scorecard = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/bench-results/challenge-scorecard.md");
    let raw = fs::read_to_string(&scorecard).expect("challenge-scorecard.md");
    let engine = pinned_engine();
    let driver = lurien::version::crate_version();
    let mut carried = std::collections::BTreeSet::new();
    for row in raw.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let kind = cells.get(1).copied().unwrap_or_default().trim_matches('`');
        if !lurien::challenge::CLAIMED_KINDS.contains(&kind) {
            continue;
        }
        carried.insert(kind.to_string());
        let named_engine = cells.get(4).copied().unwrap_or_default();
        let named_driver = cells.get(5).copied().unwrap_or_default();
        if let Some(pinned) = engine.as_deref() {
            assert_eq!(
                named_engine, pinned,
                "the {kind} claim was proved on browser {named_engine} and this tree builds \
                 {pinned}; re-run its proof and rewrite the row"
            );
        }
        assert_eq!(
            minor_of(named_driver),
            minor_of(driver),
            "the {kind} claim was proved by driver {named_driver} and this tree ships {driver}; \
             re-run its proof and rewrite the row"
        );
    }
    // A kind may carry two rows, a fixture and a live vendor, because they prove
    // different things. What may not happen is a claimed kind carrying none.
    for kind in lurien::challenge::CLAIMED_KINDS {
        assert!(
            carried.contains(*kind),
            "kind {kind} is claimed with no scorecard row to carry a build"
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

/// Any string array in `_schema.toml`, read at run time so the schema stays the
/// single list and a test cannot drift from it.
fn schema_list(key: &str) -> Vec<String> {
    let raw = fs::read_to_string(kinds_dir().join("_schema.toml")).expect("_schema.toml");
    let mut lines = raw.lines();
    while let Some(line) = lines.next() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        // An array may be written on one line or spread over several. Collect until
        // the closing bracket so the schema stays readable without a test caring.
        let mut inner = rest.trim().trim_start_matches('[').to_string();
        while !inner.contains(']') {
            let Some(next) = lines.next() else {
                break;
            };
            inner.push(' ');
            inner.push_str(next.trim());
        }
        let inner = inner.split(']').next().unwrap_or_default().to_string();
        let values: Vec<String> = inner
            .split(',')
            .map(|v| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
            .collect();
        assert!(!values.is_empty(), "schema key {key} lists nothing");
        return values;
    }
    panic!("schema has no key {key}");
}

/// A computed kind is executed from its `[work]` table. A row that omits it, or
/// names an algorithm, a difficulty format, or an address form no primitive
/// implements, is a vendor that would fail on a live page in silence.
#[test]
fn every_pow_binding_carries_a_work_table_the_engine_can_execute() {
    let algos = schema_list("pow_algos");
    let formats = schema_list("pow_formats");
    let reads = schema_list("work_read_forms");
    let submits = schema_list("work_submit_forms");
    let mut checked = 0;
    for binding in lurien::catalog::CATALOG {
        if binding.kind != "pow" {
            assert!(
                binding.work.is_empty(),
                "{} is kind {} and carries a work table nothing will execute",
                binding.name,
                binding.kind
            );
            continue;
        }
        checked += 1;
        let work: std::collections::BTreeMap<&str, &str> = binding.work.iter().copied().collect();
        for key in ["algo", "format", "challenge", "difficulty", "submit"] {
            assert!(
                work.contains_key(key),
                "pow binding {} has no work.{key}",
                binding.name
            );
        }
        assert!(
            algos.iter().any(|a| a == work["algo"]),
            "pow binding {} names algo {}, which is not in pow_algos",
            binding.name,
            work["algo"]
        );
        assert!(
            formats.iter().any(|f| f == work["format"]),
            "pow binding {} names format {}, which is not in pow_formats",
            binding.name,
            work["format"]
        );
        for key in ["challenge", "difficulty", "salt", "prefix"] {
            let Some(address) = work.get(key) else {
                continue;
            };
            assert!(
                reads.iter().any(|form| address.starts_with(form.as_str())),
                "pow binding {} reads work.{key} as {address}, which is no known address form",
                binding.name
            );
        }
        assert!(
            submits
                .iter()
                .any(|form| work["submit"].starts_with(form.as_str())),
            "pow binding {} submits to {}, which is no known address form",
            binding.name,
            work["submit"]
        );
    }
    assert!(checked > 0, "no pow binding in the catalog to check");
}

/// The work table has to survive the trip to the engine. It arrives as JSON in
/// `LURIEN_CHALLENGE`, and a table dropped there is a pow row the engine refuses.
#[test]
fn the_engine_config_carries_the_work_table_for_every_pow_binding() {
    let config = lurien::challenge::ChallengeConfig::for_process();
    let value: serde_json::Value =
        serde_json::from_str(&config.to_env_value()).expect("config is json");
    let rows = value["catalog"].as_array().expect("catalog array");
    let mut checked = 0;
    for row in rows {
        if row["kind"] != "pow" {
            continue;
        }
        checked += 1;
        let work = row["work"].as_object().expect("pow row carries work");
        for key in ["algo", "format", "challenge", "difficulty", "submit"] {
            assert!(
                work.get(key).and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty()),
                "pow row {} reached the engine without work.{key}: {row}",
                row["name"]
            );
        }
    }
    assert!(checked > 0, "no pow row reached the engine config");
}

/// A slider is measured on one element and dragged on another. A binding that
/// names only one of them drags the puzzle, which moves nothing on a live widget
/// and produces a refusal that looks like a bad measurement.
#[test]
fn every_slider_binding_names_the_element_a_hand_grabs() {
    let mut checked = 0;
    for binding in lurien::catalog::CATALOG {
        if binding.kind != "slider" {
            continue;
        }
        checked += 1;
        assert!(
            !binding.handle.is_empty(),
            "slider binding {} names no handle, so the drag would start on the puzzle",
            binding.name
        );
        assert_ne!(
            binding.handle, binding.target,
            "slider binding {} drags the element it measures",
            binding.name
        );
    }
    assert!(checked > 0, "no slider binding in the catalog to check");
}

/// The handle has to reach the engine, which reads it out of `LURIEN_CHALLENGE`.
#[test]
fn the_engine_config_carries_the_handle_for_every_slider_binding() {
    let config = lurien::challenge::ChallengeConfig::for_process();
    let value: serde_json::Value =
        serde_json::from_str(&config.to_env_value()).expect("config is json");
    let mut checked = 0;
    for row in value["catalog"].as_array().expect("catalog array") {
        if row["kind"] != "slider" {
            continue;
        }
        checked += 1;
        assert!(
            row["handle"].as_str().is_some_and(|v| !v.is_empty()),
            "slider row {} reached the engine without a handle: {row}",
            row["name"]
        );
    }
    assert!(checked > 0, "no slider row reached the engine config");
}

/// Is this something the engine's `#locate` can resolve?
///
/// A form from `target_forms`, or a CSS selector, which is any value with no
/// space in it. Prose describing an element is not executable, and a binding whose
/// kind this build claims must be executable or the solve fails on a live page
/// with "target not found".
fn executable_target(value: &str, forms: &[String]) -> bool {
    !value.is_empty()
        && (forms.iter().any(|form| value.starts_with(form.as_str()))
            || !value.contains(' '))
}

/// Claiming a kind means the engine drives it. A claimed binding whose target is
/// prose passes every other test in this file and then finds nothing on the page,
/// which is the failure this closes for every claimed kind at once, not only for
/// the one that prompted it.
#[test]
fn every_claimed_binding_names_a_target_the_engine_can_resolve() {
    let forms = schema_list("target_forms");
    assert!(!forms.is_empty(), "_schema.toml lists no target_forms");
    let claimed = lurien::challenge::CLAIMED_KINDS;
    let mut checked = 0;
    for binding in lurien::catalog::CATALOG {
        if !claimed.contains(&binding.kind) {
            continue;
        }
        // `score` is decided before paint: there is no element to act on, and the
        // binding says so in prose on purpose.
        if binding.kind == "score" || binding.kind == "pow" {
            continue;
        }
        checked += 1;
        assert!(
            executable_target(binding.target, &forms),
            "{} is a claimed {} and its target {:?} is prose, not something the engine can resolve",
            binding.name,
            binding.kind,
            binding.target
        );
        if !binding.handle.is_empty() {
            assert!(
                executable_target(binding.handle, &forms),
                "{} is a claimed {} and its handle {:?} is prose",
                binding.name,
                binding.kind,
                binding.handle
            );
        }
    }
    assert!(checked > 0, "no claimed interactive binding to check");
}

/// A `token` table is how a solve becomes observable, so a key nobody reads is a
/// vendor whose success can never be seen. The failure is silent on a live page:
/// the widget clears, nothing is observed, and the run is reported as refused. So
/// every key a binding names is checked against the schema, and every channel the
/// schema lists is checked against the parser that has to read it.
#[test]
fn every_token_channel_a_binding_names_is_one_the_engine_reads() {
    let channels = schema_list("token_channels");
    let build = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build.rs"),
    )
    .expect("build.rs");
    for channel in &channels {
        assert!(
            build.contains(&format!("\"{channel}\"")),
            "the schema lists token channel {channel} and build.rs never reads it"
        );
    }
    let mut checked = 0;
    for path in vendor_files() {
        let raw = fs::read_to_string(&path).expect("vendor toml");
        for line in raw.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("token") else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let inner = rest.trim().trim_start_matches('{').trim_end_matches('}');
            let mut named = 0;
            for pair in inner.split(',') {
                let Some((key, _)) = pair.split_once('=') else {
                    continue;
                };
                let key = key.trim();
                assert!(
                    channels.contains(&key.to_string()),
                    "{} names token channel {key}, which the schema does not list",
                    path.display()
                );
                named += 1;
            }
            assert!(
                named > 0,
                "{} has a token table that names no channel",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no vendor binding carries a token table");
}
