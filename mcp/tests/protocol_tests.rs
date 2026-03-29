//! MCP protocol tests.
//!
//! Verify JSON-RPC message handling and tool definitions by running the
//! raiju-mcp binary as a subprocess and feeding it JSON-RPC messages over stdin.

use std::io::Write;
use std::process::{Command, Stdio};

/// Helper: spawn the raiju-mcp binary, send a line to stdin, close stdin,
/// and return stdout as a string.
fn send_jsonrpc(input: &str) -> String {
    let child = Command::new("cargo")
        .args(["run", "-p", "raiju-mcp", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    let Ok(mut child) = child else {
        panic!("failed to spawn raiju-mcp binary");
    };

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(input.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Helper: send JSON-RPC and parse the first line of stdout as JSON.
fn send_and_parse(input: &str) -> serde_json::Value {
    let stdout = send_jsonrpc(input);
    let first_line = stdout.lines().next().unwrap_or("");
    serde_json::from_str(first_line).unwrap_or_else(|e| {
        panic!("failed to parse JSON-RPC response: {e}\nraw output: {stdout}");
    })
}

/// Helper: send multiple lines at once and parse each response line.
fn send_multi_and_parse(input: &str) -> Vec<serde_json::Value> {
    let child = Command::new("cargo")
        .args(["run", "-p", "raiju-mcp", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    let Ok(mut child) = child else {
        panic!("failed to spawn raiju-mcp binary");
    };

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(input.as_bytes()).unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| {
            panic!("failed to parse line: {e}\nline: {l}");
        }))
        .collect()
}

// -------------------------------------------------------
// initialize method
// -------------------------------------------------------

#[test]
fn initialize_returns_server_info() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#);

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["error"].is_null(), "should not have error");
    assert_eq!(resp["result"]["serverInfo"]["name"], "raiju-mcp");
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert!(
        resp["result"]["capabilities"]["tools"].is_object(),
        "should declare tools capability"
    );
}

#[test]
fn initialize_preserves_string_id() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"initialize","id":"my-init-42"}"#);
    assert_eq!(resp["id"], "my-init-42");
    assert!(resp["result"]["serverInfo"]["name"].is_string());
}

#[test]
fn initialize_preserves_null_id() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"initialize","id":null}"#);
    assert!(resp["id"].is_null());
    assert!(resp["result"].is_object());
}

// -------------------------------------------------------
// tools/list method
// -------------------------------------------------------

#[test]
fn tools_list_returns_tool_definitions() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#);

    assert_eq!(resp["id"], 2);
    assert!(resp["error"].is_null());

    let tools = resp["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 20, "expected >= 20 tools, got {}", tools.len());

    // Verify each tool has the expected MCP structure
    for tool in tools {
        assert!(tool["name"].is_string());
        assert!(tool["description"].is_string());
        assert!(tool["inputSchema"].is_object());
    }
}

#[test]
fn tools_list_contains_critical_tools() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"tools/list","id":3}"#);

    let tools = resp["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    // Core agent workflow tools
    assert!(names.contains(&"raiju_health"), "missing raiju_health");
    assert!(names.contains(&"raiju_list_markets"), "missing raiju_list_markets");
    assert!(names.contains(&"raiju_market_detail"), "missing raiju_market_detail");
    assert!(names.contains(&"raiju_deposit"), "missing raiju_deposit");
    assert!(names.contains(&"raiju_commit"), "missing raiju_commit");
    assert!(names.contains(&"raiju_reveal"), "missing raiju_reveal");
    assert!(names.contains(&"raiju_trade"), "missing raiju_trade");
    assert!(names.contains(&"raiju_leaderboard"), "missing raiju_leaderboard");
    assert!(names.contains(&"raiju_my_status"), "missing raiju_my_status");
    assert!(names.contains(&"raiju_my_positions"), "missing raiju_my_positions");
    assert!(names.contains(&"raiju_my_payouts"), "missing raiju_my_payouts");
    // Nostr identity (ADR-028)
    assert!(names.contains(&"raiju_nostr_bind"), "missing raiju_nostr_bind");
    assert!(names.contains(&"raiju_nostr_unbind"), "missing raiju_nostr_unbind");
}

// -------------------------------------------------------
// tools/call method - error handling
// -------------------------------------------------------

#[test]
fn tools_call_unknown_tool_returns_error_in_content() {
    let input = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        },
        "id": 10
    });
    let resp = send_and_parse(&serde_json::to_string(&input).unwrap());

    assert_eq!(resp["id"], 10);
    // MCP spec: tool call errors return success with isError=true in content
    assert!(resp["error"].is_null(), "should not be JSON-RPC error");
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Unknown tool"), "error text should mention unknown tool, got: {text}");
}

#[test]
fn tools_call_empty_tool_name_returns_error_in_content() {
    let input = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "",
            "arguments": {}
        },
        "id": 11
    });
    let resp = send_and_parse(&serde_json::to_string(&input).unwrap());

    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn tools_call_missing_name_uses_empty_string() {
    // When "name" is absent, the code defaults to ""
    let input = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "arguments": {}
        },
        "id": 12
    });
    let resp = send_and_parse(&serde_json::to_string(&input).unwrap());

    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn tools_call_missing_arguments_defaults_to_empty_object() {
    // When "arguments" is absent, the code defaults to {}
    let input = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "raiju_deposit"
        },
        "id": 13
    });
    let resp = send_and_parse(&serde_json::to_string(&input).unwrap());

    // Should fail because market_id is missing, but not crash
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("market_id"), "should mention missing market_id");
}

#[test]
fn tools_call_commit_validates_prediction_bps_range() {
    let input = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "raiju_commit",
            "arguments": {
                "market_id": "test-market",
                "prediction_bps": 10001
            }
        },
        "id": 14
    });
    let resp = send_and_parse(&serde_json::to_string(&input).unwrap());

    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("0-10000"), "should mention valid range");
}

// -------------------------------------------------------
// Unknown method
// -------------------------------------------------------

#[test]
fn unknown_method_returns_error_32601() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"unknown/method","id":20}"#);

    assert_eq!(resp["id"], 20);
    assert!(resp["result"].is_null(), "should not have result");
    assert_eq!(resp["error"]["code"], -32601);
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("Method not found"),
        "error message should say 'Method not found', got: {msg}"
    );
}

#[test]
fn unknown_method_includes_method_name_in_error() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"completions/list","id":21}"#);

    assert_eq!(resp["error"]["code"], -32601);
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(msg.contains("completions/list"), "error should include the method name");
}

// -------------------------------------------------------
// Invalid JSON input
// -------------------------------------------------------

#[test]
fn invalid_json_returns_parse_error_32700() {
    let resp = send_and_parse("this is not json");

    assert!(resp["id"].is_null(), "id should be null for parse errors");
    assert_eq!(resp["error"]["code"], -32700);
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(msg.contains("Parse error"), "should contain 'Parse error', got: {msg}");
}

#[test]
fn empty_json_object_returns_error() {
    // {} is valid JSON but missing required fields for JsonRpcRequest
    let resp = send_and_parse("{}");

    // serde will fail to deserialize because "method" is missing
    assert_eq!(resp["error"]["code"], -32700);
}

#[test]
fn json_array_returns_parse_error() {
    // JSON arrays are not valid JSON-RPC requests (batch mode not supported)
    let resp = send_and_parse("[1,2,3]");
    assert_eq!(resp["error"]["code"], -32700);
}

#[test]
fn truncated_json_returns_parse_error() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"init"#);
    assert_eq!(resp["error"]["code"], -32700);
}

// -------------------------------------------------------
// Notification filtering
// -------------------------------------------------------

#[test]
fn notifications_prefix_produces_no_response() {
    // Notifications start with "notifications/" and should produce no output
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"id\":null}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":99}\n"
    );
    let responses = send_multi_and_parse(input);

    // Only the initialize request should produce a response
    assert_eq!(responses.len(), 1, "notification should not produce a response");
    assert_eq!(responses[0]["id"], 99);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "raiju-mcp");
}

#[test]
fn notifications_cancelled_produces_no_response() {
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"id\":null}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":42}\n"
    );
    let responses = send_multi_and_parse(input);

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], 42);
}

// -------------------------------------------------------
// Empty lines are skipped
// -------------------------------------------------------

#[test]
fn empty_lines_are_ignored() {
    let input = concat!(
        "\n",
        "   \n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":50}\n",
        "\n",
    );
    let responses = send_multi_and_parse(input);

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], 50);
}

// -------------------------------------------------------
// Multiple requests in sequence
// -------------------------------------------------------

#[test]
fn multiple_requests_produce_ordered_responses() {
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":1}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":2}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"unknown/foo\",\"id\":3}\n"
    );
    let responses = send_multi_and_parse(input);

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "raiju-mcp");
    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1]["result"]["tools"].is_array());
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["error"]["code"], -32601);
}

#[test]
fn parse_error_does_not_kill_the_server() {
    // A parse error on one line should not prevent processing the next line
    let input = concat!(
        "bad json here\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":77}\n"
    );
    let responses = send_multi_and_parse(input);

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 77);
    assert!(responses[1]["result"].is_object());
}

// -------------------------------------------------------
// JSON-RPC response structure
// -------------------------------------------------------

#[test]
fn all_responses_have_jsonrpc_2_0() {
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":1}\n",
        "bad json\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"unknown\",\"id\":2}\n"
    );
    let responses = send_multi_and_parse(input);

    for resp in &responses {
        assert_eq!(resp["jsonrpc"], "2.0", "every response must have jsonrpc: 2.0");
    }
}

#[test]
fn success_response_has_result_no_error() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#);
    assert!(resp["result"].is_object());
    // error should be absent (serialized with skip_serializing_if = None)
    assert!(resp.get("error").map_or(true, |e| e.is_null()));
}

#[test]
fn error_response_has_error_no_result() {
    let resp = send_and_parse(r#"{"jsonrpc":"2.0","method":"unknown","id":1}"#);
    assert!(resp["error"].is_object());
    // result should be absent
    assert!(resp.get("result").map_or(true, |r| r.is_null()));
}
