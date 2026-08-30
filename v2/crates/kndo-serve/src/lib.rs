//! The second facade frontend: an MCP server at skeleton size — one tool, `check`,
//! answering with the same Report envelope the CLI prints. Its job in the
//! architecture is to exist: two frontends consuming `kndo::<Name>` and nothing
//! deeper is the living proof the facade suffices (v1 had one consumer, and it
//! drifted three times). Transport is MCP's stdio shape — newline-delimited
//! JSON-RPC 2.0 — hand-rolled over serde_json; the protocol surface this skeleton
//! speaks fits in one match.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// Read requests line by line, answer each; notifications get no reply. Returns
/// when the input ends.
pub fn serve(reader: impl BufRead, mut writer: impl Write) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            writeln!(writer, "{response}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

/// One message in, at most one message out — the whole server as a pure function,
/// which is exactly what the tests drive.
pub fn handle_line(line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error_response(Value::Null, -32700, "parse error"));
    };
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // A notification (no id) never gets a response, whatever the method.
    let id = id?;
    let result = match method {
        "initialize" => initialize(),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tools_call(&params),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize() -> Result<Value, (i64, String)> {
    Ok(json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "kndo-serve", "version": env!("CARGO_PKG_VERSION") },
    }))
}

fn tools_list() -> Value {
    json!({
        "tools": [{
            "name": "check",
            "description": "Analyze a project with kndo and return its full report \
                            (findings, abstentions, suppressions, diagnostics) as JSON.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Project root to analyze (defaults to the server's working directory)."
                    }
                },
                "required": []
            }
        }]
    })
}

/// The one query: open a session through the facade, analyze, hand back the
/// envelope. A refusal is a tool-level error (`isError`), not a protocol error —
/// the conversation stays alive.
fn tools_call(params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name != "check" {
        return Err((-32602, format!("unknown tool: {name}")));
    }
    let path = params
        .get("arguments")
        .and_then(|a| a.get("path"))
        .and_then(Value::as_str)
        .unwrap_or(".");
    let report = match run_check(path) {
        Ok(report) => report,
        Err(refusal) => {
            return Ok(json!({
                "content": [{ "type": "text", "text": format!("refused: {refusal}") }],
                "isError": true,
            }));
        }
    };
    Ok(json!({
        "content": [{ "type": "text", "text": report }],
        "isError": false,
    }))
}

fn run_check(path: &str) -> Result<String, kndo::Refusal> {
    let session = kndo::open(path, kndo::Config::default())?;
    let snapshot = session.analyze(kndo::RunMode::Full)?;
    Ok(snapshot.report().to_json())
}
