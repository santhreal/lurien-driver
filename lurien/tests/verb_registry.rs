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

/// The guidance and return sentences are the only prose in a description, so an
/// empty or lazy one would pass the composition law below while telling a caller
/// nothing. A domain added without guidance fails to compile; one added with a
/// placeholder fails here.
#[test]
fn every_domain_and_output_shape_carries_real_prose() {
    for domain in Domain::all() {
        let guidance = domain.guidance();
        assert!(
            guidance.starts_with("Use this") && guidance.ends_with('.') && guidance.len() > 40,
            "{}: guidance must be one sentence telling a caller when to reach for the family: \
             {guidance}",
            domain.as_str()
        );
    }
    for output in [
        verb::OutputKind::Empty,
        verb::OutputKind::Text,
        verb::OutputKind::Json,
        verb::OutputKind::Png,
    ] {
        let returns = output.returns();
        assert!(
            returns.starts_with("Returns") && returns.ends_with('.') && returns.len() > 20,
            "{output:?}: must name what comes back: {returns}"
        );
    }
}

/// An agent picks a verb from its description alone. A description that says
/// only what the verb does leaves the choice between `text` and `snapshot`, or
/// between `click` and `press`, to a guess. Every one names when to reach for it
/// and what comes back, and both sentences are derived from the spec, so a verb
/// registered tomorrow is described the same way.
#[test]
fn every_tool_description_says_when_to_use_it_and_what_it_returns() {
    for spec in verb::registry() {
        let text = schema::full_description(spec);
        assert!(
            text.contains(spec.domain.guidance()),
            "{}: description does not say when to use a {} verb: {text}",
            spec.name,
            spec.domain.as_str()
        );
        assert!(
            text.contains(spec.output.returns()),
            "{}: description does not say what it returns: {text}",
            spec.name
        );
        assert!(
            text.starts_with(spec.summary),
            "{}: description must open with the summary, not restate it: {text}",
            spec.name
        );
        let Some(arg) = spec.arg("selector") else {
            continue;
        };
        // The resolver-backed verbs share one argument spec; a CSS-only one, like
        // the frame verbs, must not be described as accepting a description.
        let semantic = arg.help.contains("role:");
        assert_eq!(
            semantic,
            text.contains("ref:eN"),
            "{}: the description must promise the semantic forms only where the \
             argument accepts them: {text}",
            spec.name
        );
        if !semantic {
            assert!(
                text.contains("CSS only"),
                "{}: a CSS-only selector must say so: {text}",
                spec.name
            );
            continue;
        }
        let waits = spec.arg("timeout_ms").is_some();
        assert_eq!(
            waits,
            text.contains("It waits for the element"),
            "{}: the description must agree with whether the verb waits",
            spec.name
        );
        assert_eq!(
            !waits,
            text.contains("It does not wait"),
            "{}: a verb with no deadline must say it does not wait",
            spec.name
        );
    }
}

/// The MCP face must serve the composed description, not the bare summary: a
/// client that reads `tools/list` is the caller that most needs the guidance.
#[test]
fn mcp_tools_list_serves_the_composed_description() {
    let tools = lurien::mcp::tool_list();
    let tools = tools.as_array().expect("tools/list is an array");
    assert_eq!(tools.len(), verb::registry().len());
    for tool in tools {
        let name = tool["name"].as_str().expect("tool name");
        let spec = verb::lookup(name).expect("tool is a registry verb");
        assert_eq!(
            tool["description"].as_str().unwrap_or_default(),
            schema::full_description(spec),
            "{name}: tools/list description drifted from the spec"
        );
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

/// A frame argument that does not advertise the handle sends the caller back to
/// an index or a URL, which is the identity that moves under a run. Derived from
/// the registry, so a verb that takes a frame tomorrow is held to it too.
#[test]
fn every_frame_argument_offers_a_stable_handle() {
    let mut checked = 0;
    for spec in verb::registry() {
        for arg in spec.args.iter().filter(|arg| arg.name == "frame") {
            checked += 1;
            assert!(
                arg.help.contains("handle"),
                "{}: the frame argument does not offer a handle: {:?}",
                spec.name,
                arg.help
            );
        }
    }
    assert!(checked >= 4, "only {checked} verbs take a frame; the law found nothing to hold");
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
