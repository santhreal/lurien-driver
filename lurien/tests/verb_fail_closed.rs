//! Every verb fails closed without a usable engine, and every verb terminates.
//!
//! This is the whole-registry gate: it enumerates the registry at run time, so a
//! new verb is covered the moment it is added. A verb that reaches the network
//! without a page, panics on absent arguments, or waits forever fails here.
//!
//! What it does not catch: whether a verb does the right thing to a real page.
//! That is the live suite, which needs `LURIEN_BIN` and `DISPLAY`.

use lurien::verb::{self, Args};
use lurien::Session;
use serde_json::Value;
use std::time::Duration;

/// Point the resolver at a path that exists and is not an engine, so the result
/// is deterministic on a host that does have lurien installed.
fn poison_engine_env() {
    // Safety: this test binary owns its process environment and runs one test.
    std::env::set_var("LURIEN_BIN", "/nonexistent/lurien-engine");
    std::env::remove_var("REYNARD_BIN");
    std::env::remove_var("GUISE_REYNARD_BIN");
}

/// Synthesize a plausible value for every declared argument, so validation
/// passes and the verb is actually entered.
fn args_for(spec: &verb::VerbSpec) -> Args {
    let mut args = Args::new();
    for arg in spec.args {
        let value = match arg.ty {
            verb::ArgType::Str => Value::String(sample_str(spec.name, arg.name)),
            verb::ArgType::Path => Value::String("/nonexistent/path".into()),
            verb::ArgType::Int => Value::from(1),
            verb::ArgType::Float => Value::from(1.0),
            verb::ArgType::Bool => Value::Bool(false),
            verb::ArgType::StrList => Value::Array(vec![Value::String("/nonexistent".into())]),
        };
        args.set(arg.name, value);
    }
    args
}

fn sample_str(verb_name: &str, arg: &str) -> String {
    match arg {
        "url" => "http://127.0.0.1:1/".into(),
        "selector" => "#nothing".into(),
        "frame" => "main".into(),
        "script" => "1".into(),
        "key" => "Enter".into(),
        "domain" => "example.invalid".into(),
        "headers" => "{}".into(),
        _ => format!("{verb_name}-{arg}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_verb_errors_without_an_engine_and_never_hangs() {
    poison_engine_env();
    let session = Session::new();
    for spec in verb::registry() {
        let args = args_for(spec);
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            session.call(spec.name, &args),
        )
        .await
        .unwrap_or_else(|_| panic!("{} never returned; a verb must be bounded", spec.name));
        let err = match result {
            Ok(output) => panic!(
                "{} produced {output:?} with no engine; the product never degrades silently",
                spec.name
            ),
            Err(e) => e,
        };
        // Either the verb refused a synthesized argument before it ever asked
        // for a page, or the engine itself was refused. Both are closed doors;
        // what is forbidden is success, a panic, or a hang.
        let text = err.to_string();
        let refused_argument = matches!(err, lurien::Error::BadArgs { .. });
        assert!(
            refused_argument
                || text.contains("lurien")
                || text.contains("engine")
                || text.contains("profile")
                || text.contains("/nonexistent"),
            "{}: failure must name the engine or the refused argument, got {text:?}",
            spec.name
        );
        if refused_argument {
            assert!(
                text.starts_with(&format!("{}:", spec.name)),
                "{}: an argument refusal must name its verb, got {text:?}",
                spec.name
            );
        }
    }
    assert!(!session.is_open().await, "no page may survive this test");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verb_with_missing_arguments_is_refused_before_launch() {
    poison_engine_env();
    let session = Session::new();
    for spec in verb::registry() {
        if !spec.args.iter().any(|a| a.required) {
            continue;
        }
        let err = session
            .call(spec.name, &Args::new())
            .await
            .expect_err("required arguments are required")
            .to_string();
        assert!(
            err.contains("missing required argument"),
            "{}: empty arguments must be an argument error, not {err:?}",
            spec.name
        );
    }
}
