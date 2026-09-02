//! The MCP conversation end-to-end: handshake, tool discovery with the
//! generated options schema, the check tool, the query tools answering from a
//! HELD session (the amortization contract, asserted behaviorally), and the
//! protocol's error paths — all through `serve` on in-memory pipes.

use kndo_testkit::js_demo_project as demo_project;
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

fn call(id: u32, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": name, "arguments": arguments}})
}

fn text_of(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("text content")
}

#[test]
fn handshake_discovery_and_the_check_tool() {
    let p = demo_project();
    let root = p.root().to_string_lossy().into_owned();

    let responses = conversation(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "2025-06-18", "capabilities": {}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        call(3, "check", json!({"path": root})),
    ]);

    // The notification got no reply: three responses for four messages.
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "kndo-serve");

    // check + the seven query verbs, named by the contract's own spelling.
    let tools = responses[1]["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "check", "find", "describe", "uses", "used-by", "trace", "impact", "explain"
        ]
    );
    // A verb tool advertises the query contract's own generated Options shape:
    // the kebab-case `if-deleted` key exists and its `$defs` resolve locally.
    let impact = &tools[6];
    assert_eq!(impact["name"], "impact");
    let schema = &impact["inputSchema"];
    assert!(
        schema["properties"]["options"]["properties"]["if-deleted"].is_object(),
        "{schema}"
    );
    assert!(schema["$defs"]["ReachColor"].is_object(), "{schema}");
    assert_eq!(schema["required"], json!(["inputs"]));

    // check answers in the agent grammar — the render built for this consumer.
    let text = text_of(&responses[2]);
    assert_eq!(responses[2]["result"]["isError"], false);
    assert!(
        text.starts_with("kndo agent format 2 (kndo-v2/m6)\n"),
        "{text}"
    );
    assert!(text.contains("[kndo-"), "{text}");
    assert!(text.contains("unused"), "{text}");
}

#[test]
fn query_tools_answer_from_the_held_session_until_check_refreshes() {
    let p = demo_project();
    let root = p.root().to_string_lossy().into_owned();

    // Drive the server directly so the filesystem edit lands mid-conversation:
    // the first query analyzes and holds; the edit is invisible until check
    // refreshes — that IS the amortization contract the descriptions promise.
    let mut server = kndo_serve::Server::default();
    let mut ask = |message: Value| server.handle_line(&message.to_string()).expect("response");

    // The dead symbol has no keepers; the live one describes.
    let dead = ask(call(
        1,
        "used-by",
        json!({"path": root, "inputs": ["src/orphan.js#floats"]}),
    ));
    assert!(
        text_of(&dead).contains("kept-by: nothing"),
        "{}",
        text_of(&dead)
    );
    let live = ask(call(
        2,
        "describe",
        json!({"path": root, "inputs": ["src/used.js#used"]}),
    ));
    assert!(
        text_of(&live).contains("[src/used.js#used]"),
        "{}",
        text_of(&live)
    );

    // Edit the tree: the orphan disappears from disk…
    std::fs::remove_file(p.root().join("src/orphan.js")).expect("removable");

    // …but the held session still answers the OLD truth.
    let stale = ask(call(
        3,
        "describe",
        json!({"path": root, "inputs": ["src/orphan.js#floats"]}),
    ));
    assert!(
        text_of(&stale).contains("[src/orphan.js#floats]"),
        "held sessions answer the last analysis: {}",
        text_of(&stale)
    );

    // check refreshes; the same selector is now honestly not-found.
    let refreshed = ask(call(4, "check", json!({"path": root})));
    assert_eq!(refreshed["result"]["isError"], false);
    let after = ask(call(
        5,
        "describe",
        json!({"path": root, "inputs": ["src/orphan.js#floats"]}),
    ));
    assert!(
        text_of(&after).contains("not-found: src/orphan.js#floats"),
        "check refreshed the session: {}",
        text_of(&after)
    );
}

#[test]
fn bad_arguments_are_tool_errors_and_the_conversation_stays_alive() {
    let p = demo_project();
    let root = p.root().to_string_lossy().into_owned();

    let responses = conversation(&[
        // An unknown options key is a typo and refuses — same law as the CLI
        // and the config file.
        call(
            1,
            "find",
            json!({"path": root, "inputs": ["used"],
                               "options": {"kindd": "function"}}),
        ),
        // Missing inputs is a tool error, not a protocol error.
        call(2, "trace", json!({"path": root})),
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    ]);
    assert_eq!(responses[0]["result"]["isError"], true);
    assert!(
        text_of(&responses[0]).contains("options:"),
        "{}",
        text_of(&responses[0])
    );
    assert_eq!(responses[1]["result"]["isError"], true);
    assert!(
        text_of(&responses[1]).contains("inputs:"),
        "{}",
        text_of(&responses[1])
    );
    assert_eq!(responses[2]["result"], json!({}));
}

#[test]
fn protocol_errors_and_tool_refusals_stay_in_band() {
    let responses = conversation(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "no/such/method"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "not-a-tool"}}),
        call(3, "check", json!({"path": "/definitely/not/a/root"})),
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
