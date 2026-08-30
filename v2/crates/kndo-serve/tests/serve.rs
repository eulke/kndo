//! The MCP conversation end-to-end: handshake, tool discovery, the check tool over
//! a real project, and the protocol's error paths — all through `serve` on
//! in-memory pipes.

use kndo_testkit::TempProject;
use serde_json::{Value, json};

fn conversation(lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    kndo_serve::serve(input.as_bytes(), &mut output).expect("serve runs");
    String::from_utf8(output)
        .expect("utf8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("each response is JSON"))
        .collect()
}

#[test]
fn handshake_discovery_and_the_check_tool() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file("src/index.js", "export function api() { return 1; }\n");
    p.file("src/orphan.js", "export function floats() {}\n");
    let root = p.root().to_string_lossy().into_owned();

    let responses = conversation(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "2025-06-18", "capabilities": {}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
               "params": {"name": "check", "arguments": {"path": root}}}),
    ]);

    // The notification got no reply: three responses for four messages.
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "kndo-serve");
    assert_eq!(responses[1]["result"]["tools"][0]["name"], "check");

    let call = &responses[2]["result"];
    assert_eq!(call["isError"], false);
    let text = call["content"][0]["text"].as_str().expect("text content");
    let report: Value = serde_json::from_str(text).expect("the tool returns the Report envelope");
    assert_eq!(report["run"]["schema"], "kndo-v2/m3");
    assert!(
        report["findings"]
            .as_array()
            .is_some_and(|f| f.iter().any(|x| x["category"] == "unused"))
    );
}

#[test]
fn protocol_errors_and_tool_refusals_stay_in_band() {
    let responses = conversation(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "no/such/method"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "not-a-tool"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
               "params": {"name": "check", "arguments": {"path": "/definitely/not/a/root"}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32601);
    assert_eq!(responses[1]["error"]["code"], -32602);
    // A refusal is a tool-level error: the conversation stays alive.
    assert_eq!(responses[2]["result"]["isError"], true);
    assert_eq!(responses[3]["result"], json!({}));
}

#[test]
fn garbage_input_answers_a_parse_error_and_keeps_serving() {
    let input = "this is not json\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n";
    let mut output = Vec::new();
    kndo_serve::serve(input.as_bytes(), &mut output).expect("serve survives");
    let responses: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 9);
}
