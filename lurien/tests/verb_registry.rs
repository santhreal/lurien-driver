//! Registry invariants. These are the rules that keep a thousand verbs honest,
//! and every one of them is derived from the registry at run time: a new verb
//! that breaks a naming, documentation, or wiring law turns this suite red
//! without anyone remembering to add it to a list.

use lurien::verb::{self, schema, ArgType, Domain};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn verb_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/verb")
}

fn docs_verbs_md() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/VERBS.md")
}

#[test]
fn registry_is_not_empty() {
    assert!(
        verb::registry().len() >= 20,
        "the registry is the product; it should not shrink silently"
    );
}

#[test]
fn no_two_verbs_claim_the_same_name_or_alias() {
    assert!(
        verb::conflicts().is_empty(),
        "duplicate verb tokens: {:?}",
        verb::conflicts()
    );
}

#[test]
fn every_verb_carries_a_dotted_domain_alias() {
    for spec in verb::registry() {
        let prefix = format!("{}.", spec.domain.as_str());
        assert!(
            spec.aliases.iter().any(|a| a.starts_with(&prefix)),
            "{} has no {prefix}* alias; the dotted form is how a verb stays \
             addressable once short names get crowded",
            spec.name
        );
    }
}

#[test]
fn every_verb_and_argument_is_documented() {
    for spec in verb::registry() {
        assert!(
            spec.summary.len() > 15 && !spec.summary.ends_with(' '),
            "{}: summary is the help text, the MCP description, and the docs row",
            spec.name
        );
        for arg in spec.args {
            assert!(
                arg.help.len() > 5,
                "{}: argument {} needs help text; it is rendered in three faces",
                spec.name,
                arg.name
            );
            assert!(
                arg.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}: argument {} must be lower_snake so JSON, CLI, and HTTP agree",
                spec.name,
                arg.name
            );
        }
    }
}

#[test]
fn required_arguments_come_before_optional_ones() {
    // Required arguments are CLI positionals in declaration order. Interleaving
    // an optional one silently reorders the CLI.
    for spec in verb::registry() {
        let mut seen_optional = false;
        for arg in spec.args {
            if arg.required {
                assert!(
                    !seen_optional,
                    "{}: required argument {} follows an optional one",
                    spec.name,
                    arg.name
                );
            } else {
                seen_optional = true;
            }
        }
    }
}

#[test]
fn a_required_argument_never_has_a_default() {
    for spec in verb::registry() {
        for arg in spec.args {
            assert!(
                !(arg.required && arg.default.is_some()),
                "{}: {} is required and defaulted, which cannot both be true",
                spec.name,
                arg.name
            );
        }
    }
}

#[test]
fn every_verb_file_is_registered_in_its_domain() {
    // A verb file that nobody added to `SPECS` is dead code that looks shipped.
    for domain in Domain::all() {
        let dir = verb_src_dir().join(domain.as_str());
        let mut files = BTreeSet::new();
        for entry in fs::read_dir(&dir).expect("domain dir") {
            let path = entry.expect("entry").path();
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("file stem")
                .to_string();
            if name == "mod" {
                continue;
            }
            assert!(
                fs::read_to_string(&path)
                    .expect("verb file")
                    .contains("pub static SPEC: VerbSpec"),
                "{}: every file in a domain is exactly one verb",
                path.display()
            );
            files.insert(name);
        }
        let mod_rs = fs::read_to_string(dir.join("mod.rs")).expect("mod.rs");
        for file in &files {
            assert!(
                mod_rs.contains(&format!("&{file}::SPEC")),
                "{}/{file}.rs is not listed in SPECS",
                domain.as_str()
            );
        }
        assert_eq!(
            files.len(),
            verb::in_domain(*domain).len(),
            "{}: {} verb files but {} registered specs",
            domain.as_str(),
            files.len(),
            verb::in_domain(*domain).len()
        );
    }
}

#[test]
fn no_face_reaches_into_a_verb_module() {
    // Faces are transports. The moment one imports a verb module directly, the
    // CLI and the MCP server can diverge, which is the defect this layout exists
    // to prevent.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for face in [
        "src/mcp.rs",
        "src/serve.rs",
        "bins/lurien.rs",
        "bins/lurien-mcp.rs",
    ] {
        let src = fs::read_to_string(root.join(face)).expect(face);
        for domain in Domain::all() {
            let import = format!("verb::{}::", domain.as_str());
            assert!(
                !src.contains(&import),
                "{face} imports {import}; a face may only call Session::call"
            );
        }
    }
}

#[test]
fn json_schema_is_generated_for_every_verb() {
    for spec in verb::registry() {
        let schema = schema::json_schema(spec);
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "{}: an unknown argument must fail closed",
            spec.name
        );
        for arg in spec.args {
            let entry = &schema["properties"][arg.name];
            assert_eq!(
                entry["type"],
                serde_json::json!(arg.ty.json_type()),
                "{}: {} type must match the spec",
                spec.name,
                arg.name
            );
            if arg.ty == ArgType::StrList {
                assert_eq!(entry["items"]["type"], serde_json::json!("string"));
            }
        }
    }
}

#[test]
fn verbs_doc_is_current() {
    let generated = schema::markdown(verb::registry());
    let path = docs_verbs_md();
    let committed = fs::read_to_string(&path).unwrap_or_default();
    if committed != generated {
        fs::write(&path, &generated).expect("write regenerated verb reference");
        panic!(
            "{} was stale and has been regenerated; commit it",
            path.display()
        );
    }
}

#[test]
fn tree_doc_names_the_verb_layout() {
    let tree = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/TREE.md"),
    )
    .expect("TREE.md");
    assert!(
        tree.contains("src/verb/"),
        "TREE.md owns the import law; it must describe where verbs live"
    );
}
