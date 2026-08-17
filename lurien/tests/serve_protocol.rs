//! The HTTP face's protocol, without a browser.
//!
//! What this closes: a legacy command that maps onto a verb the registry does not
//! have, a translation that produces arguments the verb spec refuses, a wire
//! shape a live client would not recognize, and a health posture that reports
//! healthy with no engine.
//!
//! What it does not catch: whether a verb does the right thing to a real page.
//! That is the live suite, which needs an engine and a display.

use lurien::serve::{self, Command, Registry, Reply, BACKEND, SCHEMA_VERSION};
use lurien::verb;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Every legacy command a client may send. Derived from nothing at run time on
/// purpose: this list IS the compatibility promise, so removing a name from the
/// face must break this test rather than a deployed client.
const LEGACY_COMMANDS: &[&str] = &[
    "goto",
    "get_url",
    "dom_snapshot",
    "dom_query",
    "dom_extract",
    "get_html",
    "dom_click",
    "dom_type",
    "dom_select",
    "dom_upload",
    "dom_key",
    "dom_wait",
    "dom_screenshot",
    "dom_readonly_eval",
    "execute_js",
    "dom_frames",
    "dom_console",
    "dom_signals",
    "dom_network",
    "dom_downloads",
    "dialog",
    "mouse",
    "scroll",
    "get_cookies",
    "set_cookie",
    "get_state",
    "set_state",
    "clear_state",
    "captcha_solve",
    // dom_-prefixed aliases for ahura browser tool actions.
    "dom_get_cookies",
    "dom_set_cookie",
    "dom_delete_cookie",
    "dom_clear_cookies",
    "dom_get_state",
    "dom_set_state",
    "dom_clear_state",
    "dom_back",
    "dom_forward",
    "dom_reload",
    "dom_stop",
    "dom_get_title",
    "dom_get_source",
    "dom_network_log",
    "dom_clear_network_log",
    "dom_list_contexts",
    "dom_new_context",
    "dom_switch_context",
    "dom_close_context",
    "dom_get_local_storage",
    "dom_set_local_storage",
    "dom_clear_local_storage",
    "dom_clear_session_storage",
    "dom_set_user_agent",
    "dom_set_viewport",
    "dom_set_extra_headers",
    "dom_get_headers",
    "dom_set_header",
    "dom_delete_header",
    "dom_intercept_request",
    "dom_intercept_response",
    "dom_clear_intercepts",
];

/// Arguments rich enough for any legacy command to translate. Extra keys are
/// ignored by translation, so one table serves every command.
fn sample_args() -> HashMap<String, String> {
    HashMap::from(
        [
            ("url", "https://example.test/"),
            ("selector", "#login"),
            ("text", "hello"),
            ("value", "one"),
            ("key", "Enter"),
            ("code", "1 + 1"),
            ("path", "/etc/hostname"),
            ("x", "10"),
            ("y", "20"),
            ("dx", "0"),
            ("dy", "400"),
            ("limit", "10"),
            ("name", "sid"),
            ("domain", "example.test"),
            ("action", "accept"),
            ("snapshot", "{\"version\":1}"),
            ("ms", "50"),
            ("context_id", "ctx-2"),
            ("user_agent", "TestAgent/1.0"),
            ("width", "1280"),
            ("height", "720"),
            ("headers", "{\"X-Test\":\"1\"}"),
            ("pattern", "example.test"),
            ("body", "test-body"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
}

fn command(name: &str) -> Command {
    let body = json!({
        "schema_version": SCHEMA_VERSION,
        "command": name,
        "backend": BACKEND,
        "browser_context_id": "ctx-1",
        "role": "tester",
        "profile_id": "identity-1",
        "args": sample_args(),
    });
    serde_json::from_value(body).expect("command decodes")
}

#[test]
fn every_legacy_command_maps_onto_a_verb_that_exists() {
    for name in LEGACY_COMMANDS {
        let (target, _) = serve::translate(&command(name)).unwrap_or_else(|e| {
            panic!("{name} does not translate: {e}");
        });
        assert!(
            verb::lookup(target).is_some(),
            "{name} maps to {target}, which is not in the registry"
        );
    }
}

#[test]
fn every_translation_survives_the_verb_spec() {
    // Translation that produces an argument the verb refuses would fail at run
    // time on a live page, which is exactly the drift this face exists to avoid.
    for name in LEGACY_COMMANDS {
        let (target, args) = serve::translate(&command(name)).expect("translates");
        let spec = verb::lookup(target).expect("registered");
        args.validate(spec)
            .unwrap_or_else(|e| panic!("{name} -> {target}: {e}"));
    }
}

#[test]
fn an_unknown_command_is_refused_by_name() {
    let err = serve::translate(&command("teleport")).expect_err("unknown command");
    assert!(err.contains("teleport"), "{err}");
    assert!(err.contains("lurien verbs"), "{err}");
}

#[test]
fn a_missing_required_argument_is_refused_before_any_session_exists() {
    let mut cmd = command("goto");
    cmd.url = None;
    cmd.args = Some(HashMap::new());
    let err = serve::translate(&cmd).expect_err("url is required");
    assert!(err.contains("url"), "{err}");
}

#[test]
fn frame_scope_selects_the_frame_verb() {
    let mut cmd = command("dom_click");
    cmd.args
        .as_mut()
        .expect("args")
        .insert("frame".to_string(), "1".to_string());
    let (target, args) = serve::translate(&cmd).expect("translates");
    assert_eq!(target, "click-in");
    assert_eq!(args.opt_str("frame"), Some("1"));

    let (plain, _) = serve::translate(&command("dom_click")).expect("translates");
    assert_eq!(plain, "click");
}

#[test]
fn an_element_ref_becomes_a_selector() {
    let mut cmd = command("dom_click");
    let args = cmd.args.as_mut().expect("args");
    args.remove("selector");
    args.insert("ref".to_string(), "element:7".to_string());
    let (_, decoded) = serve::translate(&cmd).expect("translates");
    assert_eq!(
        decoded.opt_str("selector"),
        Some("[data-lurien-ref=\"7\"]")
    );
}

#[test]
fn scroll_accepts_a_direction_or_explicit_deltas() {
    let mut cmd = command("scroll");
    let args = cmd.args.as_mut().expect("args");
    args.remove("dx");
    args.remove("dy");
    args.insert("direction".to_string(), "up".to_string());
    args.insert("amount".to_string(), "120".to_string());
    let (target, decoded) = serve::translate(&cmd).expect("translates");
    assert_eq!(target, "scroll");
    assert_eq!(decoded.i64("dy", 0), -120);
    assert_eq!(decoded.i64("dx", 7), 0);

    let (_, explicit) = serve::translate(&command("scroll")).expect("translates");
    assert_eq!(explicit.i64("dy", 0), 400);
}

#[test]
fn a_bad_direction_is_refused_rather_than_scrolling_down() {
    let mut cmd = command("scroll");
    let args = cmd.args.as_mut().expect("args");
    args.remove("dx");
    args.remove("dy");
    args.insert("direction".to_string(), "sideways".to_string());
    let err = serve::translate(&cmd).expect_err("bad direction");
    assert!(err.contains("sideways"), "{err}");
}

#[tokio::test]
async fn health_answers_without_an_engine_and_says_so() {
    let degraded = Reply::health_with_engine(0, None);
    assert!(degraded.success, "the process can still answer health");
    assert_eq!(degraded.metadata["stealth_engine"], json!("missing"));
    assert_eq!(degraded.metadata["webdriver_masked"], json!(false));
    assert!(!degraded.metadata.contains_key("stealth_engine_path"));
    assert!(degraded.metadata.contains_key("warnings"));
    assert!(degraded.output.contains("DEGRADED"), "{}", degraded.output);

    let healthy = Reply::health_with_engine(2, Some("/opt/lurien/lurien"));
    assert_eq!(healthy.metadata["stealth_engine"], json!("lurien"));
    assert_eq!(healthy.metadata["webdriver_masked"], json!(true));
    assert_eq!(healthy.metadata["active_browser_contexts"], json!(2));
    assert!(!healthy.output.contains("DEGRADED"), "{}", healthy.output);
}

#[tokio::test]
async fn health_advertises_the_capability_a_client_gates_on() {
    // A deployed readiness check asserts this key. Captcha handling lives in the
    // engine, so it is true for every session this face hands out.
    let reply = Reply::health_with_engine(0, None);
    assert_eq!(reply.metadata["captcha_solve"], json!(true));
    assert_eq!(reply.metadata["schema_version"], json!(SCHEMA_VERSION));
    assert_eq!(
        reply.metadata["verbs"],
        json!(verb::registry().len()),
        "health reports the live verb count"
    );
}

#[tokio::test]
async fn the_health_route_is_reachable_under_both_paths() {
    let registry = Registry::default();
    for path in ["/health", "/v1/health", "/v1/health/", "/v1/health?probe=1"] {
        let (status, reply) = serve::route("GET", path, b"", &registry).await;
        assert_eq!(status, 200, "{path}");
        assert!(reply.success, "{path}");
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_404_and_not_a_silent_success() {
    let registry = Registry::default();
    let (status, reply) = serve::route("GET", "/v1/nope", b"", &registry).await;
    assert_eq!(status, 404);
    assert!(!reply.success);
    let (status, _) = serve::route("POST", "/v1/health", b"", &registry).await;
    assert_eq!(status, 404, "health is GET only");
}

#[tokio::test]
async fn a_wrong_schema_version_is_refused_and_names_both_versions() {
    let registry = Registry::default();
    let body = json!({
        "schema_version": 99,
        "command": "goto",
        "backend": BACKEND,
        "browser_context_id": "ctx",
    });
    let (status, reply) = serve::route(
        "POST",
        "/v1/browser/command",
        body.to_string().as_bytes(),
        &registry,
    )
    .await;
    assert_eq!(status, 200, "a refusal is a well-formed reply");
    assert!(!reply.success);
    assert!(reply.error.contains("99"), "{}", reply.error);
    assert!(
        reply.error.contains(&SCHEMA_VERSION.to_string()),
        "{}",
        reply.error
    );
}

#[tokio::test]
async fn a_wrong_backend_is_refused() {
    let registry = Registry::default();
    let body = json!({
        "schema_version": SCHEMA_VERSION,
        "command": "goto",
        "backend": "chrome",
        "browser_context_id": "ctx",
    });
    let (_, reply) = serve::route(
        "POST",
        "/v1/browser/command",
        body.to_string().as_bytes(),
        &registry,
    )
    .await;
    assert!(!reply.success);
    assert!(reply.error.contains("chrome"), "{}", reply.error);
}

#[tokio::test]
async fn malformed_json_is_a_400_with_a_reason() {
    let registry = Registry::default();
    let (status, reply) = serve::route("POST", "/v1/browser/command", b"{oops", &registry).await;
    assert_eq!(status, 400);
    assert!(reply.error.contains("invalid JSON command"), "{}", reply.error);
}

#[tokio::test]
async fn a_command_for_a_closed_context_says_how_to_open_one() {
    let registry = Registry::default();
    let reply = serve::dispatch(command("get_url"), &registry).await;
    assert!(!reply.success);
    assert!(reply.error.contains("not open"), "{}", reply.error);
    assert!(reply.error.contains("launch"), "{}", reply.error);
    assert!(registry.is_empty().await, "no session may be created");
}

#[tokio::test]
async fn closing_a_context_that_was_never_open_is_not_an_error() {
    let registry = Registry::default();
    let reply = serve::dispatch(command("close"), &registry).await;
    assert!(reply.success);
    assert_eq!(reply.metadata["closed"], json!(false));
}

#[tokio::test]
async fn launch_without_a_context_id_is_refused() {
    let registry = Registry::default();
    let body = json!({
        "schema_version": SCHEMA_VERSION,
        "command": "launch",
        "backend": BACKEND,
        "browser_context_id": "   ",
    });
    let cmd: Command = serde_json::from_value(body).expect("decodes");
    let reply = serve::dispatch(cmd, &registry).await;
    assert!(!reply.success);
    assert!(reply.error.contains("browser_context_id"), "{}", reply.error);
    assert!(registry.is_empty().await);
}

#[tokio::test]
async fn list_contexts_reports_an_empty_fleet_honestly() {
    let registry = Registry::default();
    let reply = serve::dispatch(command("list_contexts"), &registry).await;
    assert!(reply.success);
    assert_eq!(reply.metadata["count"], json!(0));
    assert_eq!(reply.metadata["contexts"], json!([]));
}

#[test]
fn the_reply_wire_shape_keeps_the_keys_a_client_reads() {
    let reply = Reply::health_with_engine(1, Some("/opt/lurien/lurien"));
    let value: Value = serde_json::to_value(&reply).expect("serializes");
    for key in ["success", "output", "metadata", "console_error_count"] {
        assert!(value.get(key).is_some(), "reply lost {key}");
    }
    // Empty optional fields stay off the wire, as they always have.
    for key in ["error", "current_url", "request_refs", "network_entries"] {
        assert!(value.get(key).is_none(), "reply leaked empty {key}");
    }
}

#[test]
fn conflicting_content_length_headers_are_refused() {
    let headers = "POST /v1/browser/command HTTP/1.1\r\nContent-Length: 42\r\nContent-Length: 100\r\n\r\n";
    let err = serve::content_length(headers).expect_err("conflict");
    assert!(err.contains("conflicting Content-Length"), "{err}");
    let agreed = "POST / HTTP/1.1\r\nContent-Length: 42\r\ncontent-length: 42\r\n\r\n";
    assert_eq!(serve::content_length(agreed).expect("agreed"), Some(42));
}

#[test]
fn a_request_line_after_blank_lines_still_parses() {
    let raw = "\r\n\r\nPOST /v1/browser/command?trace=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    let (method, path) = serve::request_line(raw).expect("parses");
    assert_eq!(method, "POST");
    assert_eq!(path, "/v1/browser/command?trace=1");
}
