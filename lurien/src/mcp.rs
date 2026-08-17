//! MCP face. A transport over [`Session::call`], nothing more.
//!
//! Tool names, descriptions, and input schemas are read off the verb registry,
//! so `lurien-mcp` and the `lurien` CLI expose one API by construction. There is
//! no per-tool code here and no `challenge` tool: captcha is a property of
//! `goto`.

use crate::error::Error;
use crate::session::Session;
use crate::verb::{self, schema, Args};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

/// MCP protocol version we speak.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Skill text. This is the only skill. There is no `SKILL.md`.
pub const MCP_DESCRIPTION: &str = "\
lurien is a Firefox you drive like Playwright. Persona is coherent from TLS \
to the pixel. Captchas are a property of goto: score-class (managed Cloudflare) \
passes because the persona holds; interactive captchas fail loud in v1. \
There is no challenge tool and no CapSolver. Engine required \
(LURIEN_BIN or ~/.local/share/lurien/lurien). v1 is Linux x86_64, headful. \
Honest leaks: matched-host Linux Firefox only.";

/// Playwright-MCP compatibility set. Every name here is a verb in the registry,
/// so a Playwright-MCP client attaches with no prompt changes. The registry is a
/// superset; these are the names we promise not to rename.
pub const TOOL_NAMES: &[&str] = &[
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
];

/// Incoming JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol literal `"2.0"`.
    pub jsonrpc: String,
    /// Request id. Absent on notifications.
    #[serde(default)]
    pub id: Option<Value>,
    /// Method name.
    pub method: String,
    /// Optional params object.
    #[serde(default)]
    pub params: Option<Value>,
}

/// Outgoing JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Protocol literal `"2.0"`.
    pub jsonrpc: String,
    /// Echoed request id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Success payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional extra data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Stdio MCP server over one [`Session`].
pub struct McpServer {
    session: Arc<Session>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    /// New server. The engine is resolved on the first verb that needs a page,
    /// not here, so `tools/list` answers before an engine exists.
    #[must_use]
    pub fn new() -> Self {
        Self {
            session: Arc::new(Session::new()),
        }
    }

    /// Serve an existing session (used by tests and by `lurien serve`).
    #[must_use]
    pub fn with_session(session: Arc<Session>) -> Self {
        Self { session }
    }

    /// The session this server drives.
    #[must_use]
    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    /// Process one JSON-RPC line.
    pub async fn handle_line(&self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let request: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(req) => req,
            Err(e) => {
                return Some(rpc_err(None, -32700, format!("Parse error: {e}")));
            }
        };
        self.handle_request(request).await
    }

    /// Process a parsed request.
    pub async fn handle_request(&self, request: JsonRpcRequest) -> Option<String> {
        let is_notification = request.id.is_none();
        let id = request.id.clone();
        let response = match request.method.as_str() {
            "initialize" => ok(
                id,
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "lurien-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": MCP_DESCRIPTION,
                }),
            ),
            "notifications/initialized" | "initialized" => return None,
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, json!({ "tools": tool_list() })),
            "tools/call" => {
                let params = request.params.unwrap_or_else(|| json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match self.call_tool(name, args).await {
                    Ok(text) => ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        }),
                    ),
                    Err(e) => ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": e.to_string() }],
                            "isError": true
                        }),
                    ),
                }
            }
            unknown => {
                if is_notification {
                    return None;
                }
                return Some(rpc_err(id, -32601, format!("Method not found: {unknown}")));
            }
        };
        Some(serde_json::to_string(&response).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"serialize failed"}}"#.into()
        }))
    }

    /// Dispatch one tool call. `challenge` is refused by name: captcha is
    /// automatic, so a client that asks for it is told so instead of being
    /// silently ignored.
    async fn call_tool(&self, name: &str, args: Value) -> Result<String, Error> {
        if name == "challenge" {
            return Err(Error::UnknownMcpTool {
                name: name.to_string(),
            });
        }
        let Some(spec) = verb::lookup(name) else {
            return Err(Error::UnknownMcpTool {
                name: name.to_string(),
            });
        };
        let args = Args::from_value(args)?;
        let output = spec.call(&self.session, &args).await?;
        Ok(output.to_text())
    }
}

/// `tools/list` payload: every registry verb, described by its own spec.
#[must_use]
pub fn tool_list() -> Value {
    let tools: Vec<Value> = verb::registry()
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "description": schema::full_description(spec),
                "inputSchema": schema::json_schema(spec),
            })
        })
        .collect();
    Value::Array(tools)
}

fn ok(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

fn rpc_err(id: Option<Value>, code: i64, message: String) -> String {
    let r = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
    };
    serde_json::to_string(&r).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"serialize failed"}}"#.into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_names_are_all_registry_verbs() {
        for name in TOOL_NAMES {
            assert!(
                verb::lookup(name).is_some(),
                "Playwright-MCP name {name} is not a verb"
            );
        }
    }

    #[test]
    fn no_tool_is_called_challenge() {
        assert!(verb::lookup("challenge").is_none());
    }
}
