//! The second facade frontend: an MCP server speaking the agent grammar — the
//! amortized-graph transport for models. `check` analyzes and refreshes the
//! held session; the seven query verbs are one tool each over the SAME
//! [`kndo::query::Request`] the CLI builds, answering from the held snapshot so
//! a conversation of many lookups pays for one analysis. Every tool returns
//! agent-format text: the token-thrifty render is the reason this server
//! exists — the JSON envelopes stay the CLI's piped door.
//!
//! Two frontends consuming `kndo::<Name>` and nothing deeper is the living
//! proof the facade suffices (v1 had one consumer, and it drifted three
//! times). Transport is MCP's stdio shape — newline-delimited JSON-RPC 2.0 —
//! hand-rolled over serde_json; the protocol surface fits in one match.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

const PROTOCOL_VERSION: &str = "2025-06-18";

/// The held sessions: one snapshot per analyzed root, answering queries until
/// `check` refreshes it. Queries read the last analysis — after editing files,
/// `check` is the refresh; the tool descriptions say so.
#[derive(Default)]
pub struct Server {
    sessions: BTreeMap<PathBuf, kndo::Snapshot>,
}

/// Read requests line by line, answer each; notifications get no reply. Returns
/// when the input ends.
pub fn serve(reader: impl BufRead, mut writer: impl Write) -> std::io::Result<()> {
    let mut server = Server::default();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&line) {
            writeln!(writer, "{response}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

impl Server {
    /// One message in, at most one message out, over the held sessions — which
    /// is exactly what the tests drive.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
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
            "tools/call" => self.tools_call(&params),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    /// Route a call: `check` analyzes and refreshes the held session; a verb
    /// tool answers from it (analyzing once when nothing is held yet). A
    /// refusal or bad argument is a tool-level error (`isError`), not a
    /// protocol error — the conversation stays alive.
    fn tools_call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_string();
        if name == "check" {
            return Ok(match self.check(&path) {
                Ok(text) => tool_text(text, false),
                Err(refusal) => tool_text(format!("refused: {refusal}"), true),
            });
        }
        let Some(verb) = VERBS.iter().copied().find(|v| v.as_str() == name) else {
            return Err((-32602, format!("unknown tool: {name}")));
        };
        Ok(match self.query(verb, &path, &arguments) {
            Ok(text) => tool_text(text, false),
            Err(problem) => tool_text(problem, true),
        })
    }

    fn check(&mut self, path: &str) -> Result<String, kndo::Refusal> {
        let snapshot = analyze(path)?;
        let text = snapshot.report().to_agent();
        self.sessions.insert(session_key(path), snapshot);
        Ok(text)
    }

    fn query(&mut self, verb: kndo::query::Verb, path: &str, arguments: &Value) -> QueryResult {
        let inputs: Vec<String> =
            serde_json::from_value(arguments.get("inputs").cloned().unwrap_or(json!([])))
                .map_err(|e| format!("inputs: {e}"))?;
        if inputs.is_empty() {
            return Err("inputs: at least one is required".to_string());
        }
        // The same deny-unknown-fields Options the CLI and the schema speak: a
        // typo refuses with serde's own message, never a silent ignore.
        let options: kndo::query::Options =
            serde_json::from_value(arguments.get("options").cloned().unwrap_or(json!({})))
                .map_err(|e| format!("options: {e}"))?;
        let snapshot = self
            .snapshot_for(path)
            .map_err(|refusal| format!("refused: {refusal}"))?;
        let response = snapshot.query(&kndo::query::Request {
            verb,
            inputs,
            options,
        });
        Ok(response.to_agent())
    }

    fn snapshot_for(&mut self, path: &str) -> Result<&kndo::Snapshot, kndo::Refusal> {
        let key = session_key(path);
        if !self.sessions.contains_key(&key) {
            let snapshot = analyze(path)?;
            self.sessions.insert(key.clone(), snapshot);
        }
        Ok(&self.sessions[&key])
    }
}

type QueryResult = Result<String, String>;

fn analyze(path: &str) -> Result<kndo::Snapshot, kndo::Refusal> {
    let session = kndo::open(path, kndo::Config::default())?;
    session.analyze(kndo::RunMode::Full)
}

/// One root, one session, however it was spelled — canonicalized when the path
/// exists; a path that doesn't will refuse before anything is held.
fn session_key(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn tool_text(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
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

const VERBS: &[kndo::query::Verb] = &[
    kndo::query::Verb::Find,
    kndo::query::Verb::Describe,
    kndo::query::Verb::Uses,
    kndo::query::Verb::UsedBy,
    kndo::query::Verb::Trace,
    kndo::query::Verb::Impact,
    kndo::query::Verb::Explain,
];

fn verb_description(verb: kndo::query::Verb) -> &'static str {
    use kndo::query::Verb;
    match verb {
        Verb::Find => {
            "Search declarations and files by name; answers with selectors to \
             feed the other tools. Ranks exact > prefix > substring; \
             options.kind and options.color narrow."
        }
        Verb::Describe => {
            "One node in full: kind, reachability color, lines, declaration or \
             file facts, a preview of what keeps it, and the findings on it."
        }
        Verb::Uses => "What this node references — its outgoing edges, with sites.",
        Verb::UsedBy => {
            "What keeps this node alive — the same evidence the unused judgment \
             counted, so an empty answer means nothing does."
        }
        Verb::Trace => {
            "Why this node is alive: the shortest path from a root (production \
             first, then test, then tooling), each hop naming its edge and \
             confidence, the in-file keeper last. A null path means NOT \
             reachable that way."
        }
        Verb::Impact => {
            "What depends on this node — the reverse closure nearest-first with \
             by-color totals; options.if-deleted additionally simulates the \
             removal and reports the reachability flips."
        }
        Verb::Explain => {
            "From a finding id (kndo-…) to its why: the finding brief plus the \
             full description of its subject."
        }
    }
}

fn verb_inputs_description(verb: kndo::query::Verb) -> &'static str {
    use kndo::query::Verb;
    match verb {
        Verb::Find => "Search patterns, one answer each.",
        Verb::Explain => "Finding ids (kndo-…), one answer each.",
        _ => "Selectors — `path`, `path#name`, `path#Owner.member` — one answer each.",
    }
}

/// The advertised input schema: `path` + `inputs` + the query contract's OWN
/// generated `Options` shape, its `$defs` hoisted to this schema's root so the
/// refs still resolve. Derived from the same type the door parses — the
/// listing cannot drift from the contract.
fn tool_input_schema(inputs_description: &str) -> Value {
    let mut options = kndo::query::options_schema();
    let defs = options
        .as_object_mut()
        .and_then(|o| o.remove("$defs"))
        .unwrap_or(json!({}));
    if let Some(o) = options.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
    }
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Project root to answer from (defaults to the server's working directory)."
            },
            "inputs": {
                "type": "array",
                "items": { "type": "string" },
                "description": inputs_description
            },
            "options": options,
        },
        "required": ["inputs"],
        "$defs": defs,
    })
}

fn tools_list() -> Value {
    let mut tools = vec![json!({
        "name": "check",
        "description": "Analyze a project with kndo and return the agent-format report \
                        (finding ids as handles, health, diagnostics). Refreshes the \
                        session the query tools answer from — call again after editing files.",
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
    })];
    for &verb in VERBS {
        tools.push(json!({
            "name": verb.as_str(),
            "description": verb_description(verb),
            "inputSchema": tool_input_schema(verb_inputs_description(verb)),
        }));
    }
    json!({ "tools": tools })
}
