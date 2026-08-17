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
    "screenshot",
    "page_screenshot",
    "choose_files",
    "dom_readonly_eval",
    "execute_js",
    "dom_frames",
    "dom_console",
    "dom_signals",
    "dom_network",
    "dom_downloads",
    "downloads",
    "download_wait",
    "download_save",
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
    // DOM tool actions (eval-based).
    "dom_eval",
    "dom_check",
    "dom_storage",
    "dom_indexeddb",
    "dom_service_workers",
    "dom_cache",
    "dom_xpath",
    "dom_attr",
    "dom_get_attributes",
    "dom_form",
    "dom_dispatch",
    "dom_wait_for",
    "dom_scroll_into_view",
    "dom_hover",
    "dom_focus",
    "dom_download",
    "dom_get_styles",
    "dom_set_style",
    "dom_mutation_observer",
    "dom_clear_mutation_observer",
    "batch",
    "dom_batch",
    "geolocation",
    "geolocation_set",
    "geolocation_clear",
    "permissions",
];

/// Arguments rich enough for any legacy command to translate. Extra keys are
/// ignored by translation, so one table serves every command.
fn sample_args() -> HashMap<String, Value> {
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
            ("steps", "wait ms=1\ntitle"),
            ("context_id", "ctx-2"),
            ("user_agent", "TestAgent/1.0"),
            ("width", "1280"),
            ("height", "720"),
            ("headers", "{\"X-Test\":\"1\"}"),
            ("pattern", "example.test"),
            ("body", "test-body"),
            ("xpath", "//div[@class='test']"),
            ("attr_name", "data-test"),
            ("attr_value", "value1"),
            ("form_action", "submit"),
            ("event_type", "click"),
            ("bubbles", "true"),
            ("cancelable", "true"),
            ("wait_selector", "#result"),
            ("wait_timeout_ms", "5000"),
            ("scroll_selector", "#target"),
            ("hover_selector", "#menu"),
            ("focus_selector", "#input"),
            ("download_url", "https://example.test/file"),
            ("checked", "true"),
            ("latitude", "52.52"),
            ("longitude", "13.405"),
            ("accuracy_m", "55"),
        ]
        .map(|(k, v)| (k.to_string(), Value::from(v))),
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

/// A client that sends a JSON number rather than a quoted one used to have that
/// argument dropped: every reader in this face expects the historical string
/// shape, so `{"ms": 50}` silently became the default and `{"latitude": 52.52}`
/// became "missing required argument". Normalising scalars at decode closes the
/// class for every command at once, so this covers a numeric argument, a boolean,
/// and one of each shape that already worked.
#[test]
fn a_scalar_sent_as_json_reaches_the_verb() {
    let numeric = json!({
        "schema_version": SCHEMA_VERSION,
        "command": "geolocation_set",
        "backend": BACKEND,
        "browser_context_id": "ctx-1",
        "args": {"latitude": 52.52, "longitude": 13.405, "accuracy_m": 30},
    });
    let command: Command = serde_json::from_value(numeric).expect("command decodes");
    let (verb_name, args) = serve::translate(&command).expect("translates");
    assert_eq!(verb_name, "geolocation-set");
    assert_eq!(args.f64("latitude", 0.0), 52.52);
    assert_eq!(args.f64("longitude", 0.0), 13.405);
    assert_eq!(args.f64("accuracy_m", 0.0), 30.0);

    let mixed = json!({
        "schema_version": SCHEMA_VERSION,
        "command": "dom_wait",
        "backend": BACKEND,
        "browser_context_id": "ctx-1",
        "args": {"ms": 50},
    });
    let command: Command = serde_json::from_value(mixed).expect("command decodes");
    let (verb_name, args) = serve::translate(&command).expect("translates");
    assert_eq!(verb_name, "wait");
    assert_eq!(args.u64("ms", 0), 50);

    let boolean = json!({
        "schema_version": SCHEMA_VERSION,
        "command": "screenshot",
        "backend": BACKEND,
        "browser_context_id": "ctx-1",
        "args": {"full_page": true},
    });
    let command: Command = serde_json::from_value(boolean).expect("command decodes");
    let (verb_name, args) = serve::translate(&command).expect("translates");
    assert_eq!(verb_name, "screenshot");
    assert!(args.bool("full_page", false));
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
        .insert("frame".to_string(), Value::from("1"));
    let (target, args) = serve::translate(&cmd).expect("translates");
    assert_eq!(target, "click-in");
    assert_eq!(args.opt_str("frame"), Some("1"));

    let (plain, _) = serve::translate(&command("dom_click")).expect("translates");
    assert_eq!(plain, "click");
}

/// A `ref` names a node from a snapshot. It used to become
/// `[data-lurien-ref="7"]`, an attribute nothing in the product ever wrote, so
/// every client that sent a ref got a selector that could not match. A handle is
/// the real address now, and both spellings a client may send reach it.
#[test]
fn an_element_ref_becomes_a_snapshot_handle() {
    for (sent, expected) in [
        ("element:7", "ref:e7"),
        ("ref:7", "ref:e7"),
        ("ref:e7", "ref:e7"),
        ("#login", "#login"),
    ] {
        let mut cmd = command("dom_click");
        let args = cmd.args.as_mut().expect("args");
        args.remove("selector");
        args.insert("ref".to_string(), Value::from(sent));
        let (_, decoded) = serve::translate(&cmd).expect("translates");
        let selector = decoded.opt_str("selector").unwrap_or_default();
        assert_eq!(selector, expected, "{sent} became {selector}");
        assert!(
            !selector.contains("data-lurien"),
            "no attribute is written to the page, so none can be selected on: {selector}"
        );
    }
}

/// A list argument has three plausible spellings on the wire, and a client that
/// picks the wrong one should not silently run a one-step batch or upload a file
/// named `["a.png"`.
#[test]
fn a_list_argument_arrives_as_an_array_or_as_text() {
    let steps = ["goto url=https://example.test/", "title"];
    let sent = |value: Value| {
        let mut cmd = command("batch");
        cmd.args
            .as_mut()
            .expect("args")
            .insert("steps".to_string(), value);
        serve::translate(&cmd).map(|(verb, args)| (verb, args.str_list("steps")))
    };

    let (verb, list) = sent(json!(steps)).expect("an array translates");
    assert_eq!(verb, "batch");
    assert_eq!(list.expect("list"), steps);

    let (_, list) = sent(json!(steps.join("\n"))).expect("one step per line translates");
    assert_eq!(list.expect("list"), steps);

    let encoded = serde_json::to_string(&steps).expect("json");
    let (_, list) = sent(json!(encoded)).expect("an encoded array translates");
    assert_eq!(list.expect("list"), steps);

    let err = sent(json!("")).expect_err("an empty batch is refused");
    assert!(err.contains("steps"), "{err}");
}

/// A file input takes several files. Sending them as an array must not become one
/// path with brackets in its name.
#[test]
fn upload_takes_several_files_as_an_array() {
    let mut cmd = command("dom_upload");
    let args = cmd.args.as_mut().expect("args");
    args.insert("files".to_string(), json!(["/tmp/a.png", "/tmp/b.png"]));
    let (verb, decoded) = serve::translate(&cmd).expect("translates");
    assert_eq!(verb, "upload");
    assert_eq!(
        decoded.str_list("files").expect("list"),
        vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()]
    );
}

#[test]
fn scroll_accepts_a_direction_or_explicit_deltas() {
    let mut cmd = command("scroll");
    let args = cmd.args.as_mut().expect("args");
    args.remove("dx");
    args.remove("dy");
    args.insert("direction".to_string(), Value::from("up"));
    args.insert("amount".to_string(), Value::from("120"));
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
    args.insert("direction".to_string(), Value::from("sideways"));
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
    assert_eq!(reply.metadata["sessions"], json!([]));
}

/// A crashed client never sends `close`, so without reaping its engine, profile
/// directory, and display stay taken for the life of the server. What must not
/// happen is reaping a session that is merely slow: use, not age, is the test, so
/// a context touched inside the window survives while its idle sibling does not.
#[tokio::test]
async fn an_abandoned_session_is_reaped_and_a_used_one_is_not() {
    let registry = Registry::default();
    for context in ["busy", "gone"] {
        let mut cmd = command("launch");
        cmd.browser_context_id = context.to_string();
        registry.open(&cmd).await.expect("session opens lazily");
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    // Reaching a session is use. This is the only difference between the two.
    registry.get("busy").await.expect("busy session is open");

    let closed = registry.reap_idle(std::time::Duration::from_millis(50)).await;
    assert_eq!(closed, vec!["gone".to_string()], "only the idle context closes");
    assert_eq!(registry.list().await, vec!["busy".to_string()]);

    // The clock is per session, so the survivor is reapable once it too goes idle.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let closed = registry.reap_idle(std::time::Duration::from_millis(50)).await;
    assert_eq!(closed, vec!["busy".to_string()]);
    assert!(registry.is_empty().await, "the fleet is empty after reaping");
}

#[tokio::test]
async fn reaping_an_empty_registry_closes_nothing() {
    let registry = Registry::default();
    assert!(registry
        .reap_idle(std::time::Duration::from_millis(0))
        .await
        .is_empty());
}

/// A client deciding whether to reuse or reopen a context needs its age, whether
/// an engine is actually running behind the name, and how long it has left.
#[tokio::test]
async fn the_session_list_reports_age_state_and_the_idle_deadline() {
    let registry = Registry::default();
    let mut cmd = command("launch");
    cmd.browser_context_id = "described".to_string();
    registry.open(&cmd).await.expect("session opens lazily");

    let reply = serve::dispatch(command("sessions"), &registry).await;
    assert!(reply.success, "{}", reply.error);
    assert_eq!(reply.metadata["count"], json!(1));
    let row = &reply.metadata["sessions"][0];
    assert_eq!(row["browser_context_id"], json!("described"));
    // Launch is lazy: a named context with no engine must not claim to have one.
    assert_eq!(row["state"], json!("named"));
    assert!(row["age_ms"].as_u64().is_some(), "age is reported: {row}");
    let idle = row["idle_ms"].as_u64().expect("idle is reported");
    let limit = reply.metadata["idle_limit_ms"].as_u64().expect("limit");
    assert_eq!(limit, serve::idle_ms());
    if limit > 0 {
        assert_eq!(
            row["reap_in_ms"].as_u64().expect("deadline"),
            limit.saturating_sub(idle),
            "the deadline must follow the idle clock: {row}"
        );
    }
    assert_eq!(row["url"], json!(""), "nothing was navigated");
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
