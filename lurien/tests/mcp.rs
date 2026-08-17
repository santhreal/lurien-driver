//! MCP: Playwright names present, challenge absent, missing engine is named.

use lurien::mcp::{McpServer, MCP_DESCRIPTION, TOOL_NAMES};
use serde_json::{json, Value};

#[tokio::test]
async fn tools_list_is_playwright_shaped() {
    let server = McpServer::new();
    let list = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });
    let resp = server
        .handle_line(&list.to_string())
        .await
        .expect("tools/list");
    let v: Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in TOOL_NAMES {
        assert!(names.contains(want), "missing tool {want}");
    }
    assert!(!names.contains(&"challenge"));
}

#[tokio::test]
async fn challenge_tool_is_unknown() {
    let server = McpServer::new();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "challenge", "arguments": {} }
    });
    let resp = server
        .handle_line(&call.to_string())
        .await
        .expect("tools/call");
    let v: Value = serde_json::from_str(&resp).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(v["result"]["isError"].as_bool().unwrap_or(false));
    assert!(
        text.contains("challenge") && text.contains("automatic"),
        "challenge refusal must name the tool and say captcha is automatic: {text}"
    );
}

#[tokio::test]
async fn initialize_carries_the_skill() {
    let server = McpServer::new();
    let init = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name":"t","version":"0"} }
    });
    let resp = server
        .handle_line(&init.to_string())
        .await
        .expect("initialize");
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["serverInfo"]["name"], "lurien-mcp");
    let instructions = v["result"]["instructions"].as_str().unwrap_or("");
    assert!(instructions.contains("Captchas are a property of goto"));
    assert!(MCP_DESCRIPTION.contains("no challenge tool"));
}
