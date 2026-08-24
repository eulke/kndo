# Security Policy

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for a security vulnerability.

Instead, use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/eulke/kondo/security) of this repository.
2. Click **"Report a vulnerability"**.
3. Fill in as much detail as you can: affected version, a reproduction (a minimal project and
   the `kndo` invocation that triggers the issue is ideal), and the impact you believe it has.

This opens a private advisory visible only to the maintainers until a fix is ready — no
information is public until it's resolved and you agree to disclosure.

If GitHub's private reporting isn't available to you for some reason, contact
**[@eulke](https://github.com/eulke)** directly via a private GitHub message.

## Scope

kndo is a static analyzer: it reads source files and never executes the code it analyzes. The
security surface that matters most is the WASM plugin/adapter sandbox — a
third-party `.wasm` component escaping its sandbox, exhausting resources beyond its fuel budget,
or reading files outside what it declares (`requested_file_access`) are all in scope and taken
seriously. Parser crashes or panics on malformed/adversarial input (tree-sitter grammars, JSON/
manifest parsing) are also in scope, even though kndo doesn't execute analyzed code — a crash on
untrusted input is still a real availability issue for anyone running kndo in CI.

## Supported Versions

Pre-1.0: only the latest released version is supported. Once 1.0 ships, this section will state
a real support window per the semver commitments kndo makes for its contract
surfaces (JSON output schema, WASM ABI, CLI surface).
