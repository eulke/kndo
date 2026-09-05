# Agents

Two doors for a model: the **agent format**, a token-thrifty text render of the
same report and query answers, and **`kndo-serve`**, an MCP server that holds
the analysis so a conversation of many lookups pays for one run.

## The agent format

`kndo check --format agent` (the terminal default for the query verbs):

```text
kndo agent format 2 (kndo-v2/m6)
result: mode full · findings 1 · carried 0 · fixed 0 · files 6/9 claimed
health: 90.0 · implicated 1 of 10 · unused 1
extensions: kndo:js-ts 6
findings:
[kndo-ea8c083aa403] warning unused · file scripts/orphan.ts · certain
  no root anchors this file and no reachable file imports it
abstained:
- test-only: no test root anchors any file in this graph
- untested: no test root anchors any file in this graph
- crap: no coverage report ingested this run
plugins:
- kndo:coverage-lcov: roots 0 · findings 0
```

Finding ids are handles: `kndo explain <id>` expands one, `kndo used-by` and
`kndo trace` answer the questions a model asks next, and every query answer
ends with a `next:` line naming them. Symbols are spelled as
`path#name · kind · color · lines`, one per line, nothing repeated — where the
name carries the signature a language spells (`Widget.size(int)`), so two
overloads are two lines and each is an address the verbs accept. The grammar
is versioned in its first line and pinned by a golden file in the test suite,
so a prompt written against it keeps working.

## `kndo-serve`

An MCP server over stdio (newline-delimited JSON-RPC 2.0). Point a client at
the binary with the project root as its working directory:

```json
{
  "mcpServers": {
    "kndo": {
      "command": "kndo-serve",
      "cwd": "/path/to/project"
    }
  }
}
```

It exposes one tool per verb — `check`, `find`, `describe`, `uses`, `used-by`,
`trace`, `impact`, `explain` — each answering in the agent format. `check`
analyzes and refreshes the held session; the query tools read the last
analysis, so after editing files, call `check` again. Every tool takes an
optional `path` for the project root; the query tools take the verb's inputs
and options under the same names the command line uses.

`kndo-serve` is built from source (`cargo install --git
https://github.com/eulke/kondo kndo-serve`); it is not in the release archive.

## What both rely on

The same facade: the CLI and the server consume the same session, the same
query request, and the same renders, so an answer over MCP is byte-for-byte
the answer the command line prints. Nothing in either door is a second
implementation of a judgment.
