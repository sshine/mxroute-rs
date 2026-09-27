---
name: mxroute
description: >-
  Exercises the mxroute MCP server that this repository builds, against the real MXroute
  account. Use when working in mxroute-rs and asked to try a tool, check what the server
  returns for a domain, mailbox, forwarder, spam setting, DNS record or quota, or to
  confirm a change to the server behaves as intended.
license: MIT OR Apache-2.0
---

# Using the MXroute server from its own repository

`.mcp.json` runs `just mcp`, which builds the server from the working tree and takes
credentials from secretspec. The tools therefore reflect uncommitted edits, and they reach
the real account — there is no fixture behind them.

Tools are named `mcp__mxroute__mxroute_*`. The plugin registers the same server under
`mcp__plugin_mxroute_mxroute__*`; if both are installed, prefer the unprefixed one here,
since that is the one built from these sources.

## What is served

Eight read-only tools by default. The tools that write appear only when `just mcp` is given
`--allow-writes`, and the reseller tools only with `--reseller`. Neither is on in
`.mcp.json`, so a session here cannot change the account by accident.

## Before relying on what a tool returns

- Reads and writes are account-wide or domain-scoped in ways that surprise people. The
  traps, and what to do about each, are written up once in
  [the plugin skill](../../../plugin/skills/email-admin/SKILL.md) — read it rather than
  rediscovering them here.
- `just mcp-tools` prints the schemas as JSON and needs no credentials, which is the
  cheapest way to see what a change to the tool surface did.
- `just mcp -- --check` verifies the credentials against the API and exits.

## When a tool misbehaves

The server is a thin layer over the `mxroute` library crate in this workspace. A wrong
result is usually the library's, not the server's, so reach for the library's own tests
first: `just test` covers both, and `just live-test` covers the API itself.
