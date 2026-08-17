//! Every launcher hits `resolve_engine`. Adding a launcher without it is red.

use lurien::launch::launch_call_sites;
use lurien::mcp::TOOL_NAMES;
use lurien::resolve::resolve_engine;

const LAUNCH_RS: &str = include_str!("../src/launch.rs");
const LIB_RS: &str = include_str!("../src/lib.rs");
const MCP_RS: &str = include_str!("../src/mcp.rs");
const CLI: &str = include_str!("../bins/lurien.rs");
const MCP_BIN: &str = include_str!("../bins/lurien-mcp.rs");

#[test]
fn every_named_launcher_is_registered() {
    let sites = launch_call_sites();
    assert!(sites.contains(&"lurien::Browser::launch"));
    assert!(sites.contains(&"lurien::Browser::launch_with_options"));
    assert!(sites.contains(&"lurien CLI"));
    assert!(sites.contains(&"lurien-mcp"));
}

#[test]
fn launch_path_calls_resolve_engine_checked() {
    assert!(
        LAUNCH_RS.contains("resolve_engine_checked"),
        "launch.rs must resolve the engine; missing binary is Err"
    );
}

#[test]
fn cli_and_mcp_bins_hit_the_resolver() {
    assert!(
        CLI.contains("resolve_engine_checked") || CLI.contains("Browser::launch"),
        "lurien CLI must resolve the engine"
    );
    assert!(
        MCP_BIN.contains("resolve_engine_checked"),
        "lurien-mcp must fail closed without an engine"
    );
}

#[test]
fn public_launchers_do_not_call_stock_firefox() {
    for src in [LAUNCH_RS, LIB_RS, CLI, MCP_BIN] {
        assert!(
            !src.contains("launch_profiled_firefox"),
            "product path must not call launch_profiled_firefox"
        );
        assert!(
            !src.contains("drive_browser"),
            "product path must not call foxdriver::drive_browser"
        );
    }
}

#[test]
fn mcp_has_playwright_names_and_no_challenge() {
    for name in [
        "goto",
        "snapshot",
        "click",
        "type",
        "fill",
        "screenshot",
        "cookies",
        "url",
        "scroll",
        "wait",
        "frames",
        "as",
    ] {
        assert!(
            TOOL_NAMES.contains(&name),
            "Playwright-MCP verb {name} missing"
        );
    }
    assert!(!TOOL_NAMES.contains(&"challenge"));
    assert!(
        MCP_RS.contains("challenge") && MCP_RS.contains("UnknownMcpTool"),
        "MCP must refuse a challenge tool by name"
    );
}

#[test]
fn resolve_engine_is_result_not_option() {
    fn assert_result(_: fn() -> Result<String, lurien::error::Error>) {}
    assert_result(resolve_engine);
}
