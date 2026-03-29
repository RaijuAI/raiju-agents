//! Raiju MCP Server (FR-002).
//!
//! Exposes Raiju API operations as MCP tools over stdio JSON-RPC.
//! AI agents (Claude, GPT, etc.) connect via the MCP protocol and get
//! typed tool definitions for deposits, forecasts, trades, and queries.
//!
//! Usage:
//!   `RAIJU_API_KEY=<key>` `RAIJU_AGENT_ID=<uuid>` raiju-mcp
//!
//! The server reads JSON-RPC messages from stdin and writes responses to stdout.
//! Configure as an MCP server in Claude Code, Cursor, or any MCP-compatible client.

mod nonce;
mod tools;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

fn main() -> Result<()> {
    let base_url =
        std::env::var("RAIJU_URL").unwrap_or_else(|_| "https://raiju.ai".to_string());
    let api_key = std::env::var("RAIJU_API_KEY").unwrap_or_default();
    let agent_id = std::env::var("RAIJU_AGENT_ID").unwrap_or_default();

    let client = tools::RaijuClient::new(&base_url, &api_key, &agent_id);

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let error = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    &format!("Parse error: {e}"),
                );
                serde_json::to_writer(&mut stdout, &error)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                continue;
            }
        };

        // JSON-RPC notifications have no id and must not receive a response.
        if request.method.starts_with("notifications/") {
            continue;
        }

        let response = handle_request(&client, &request);
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(client: &tools::RaijuClient, req: &JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            req.id.clone(),
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "raiju-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),

        "tools/list" => JsonRpcResponse::success(
            req.id.clone(),
            serde_json::json!({
                "tools": tools::tool_definitions()
            }),
        ),

        "tools/call" => {
            let params = req.params.as_ref();
            let tool_name = params.and_then(|p| p["name"].as_str()).unwrap_or("");
            let arguments =
                params.and_then(|p| p.get("arguments")).cloned().unwrap_or(serde_json::json!({}));

            match client.call_tool(tool_name, &arguments) {
                Ok(result) => JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                        }]
                    }),
                ),
                Err(e) => JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {e}")
                        }],
                        "isError": true
                    }),
                ),
            }
        }

        _ => JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            &format!("Method not found: {}", req.method),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    /// JSON-RPC version field. Deserialized for protocol correctness but not
    /// read after parsing; prefixed with `_` to suppress the dead-code warning.
    #[serde(rename = "jsonrpc")]
    _jsonrpc: Option<String>,
    method: String,
    params: Option<serde_json::Value>,
    id: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn error(id: serde_json::Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.to_string() }),
        }
    }
}
