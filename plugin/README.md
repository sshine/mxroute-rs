# MXroute for Claude Code

Manage MXroute email hosting from Claude Code: domains, mailboxes, forwarders, catch-all,
spam filtering, DNS records and quota.

This plugin registers and configures the MCP server. It does not install it, because a
plugin ships as a git repository and cannot compile or download a binary.

## Install

First the binary, on `PATH`:

```bash
nix profile install github:sshine/mxroute-rs#mxroute-mcp
```

or download the archive for your platform from [releases] and put `mxroute-mcp` somewhere on
`PATH`.

Then the plugin:

```bash
claude plugin marketplace add sshine/mxroute-rs
claude plugin install mxroute@sshine-mxroute
```

Claude Code prompts for the server hostname, the DirectAdmin username and an API key from
[the panel]. The key goes to the OS keychain rather than to a settings file.

If the tools do not appear, `/plugin` shows the startup error in its Errors tab; a missing
binary is the usual cause.

## What is served

Eight read tools by default. The **Allow changes** option adds fourteen more that create,
change and delete things; the **Reseller account** option adds ten for reseller users and
packages. Both are off to begin with, so an install pointed at the wrong account cannot
damage it.

Deleting a domain or a reseller user asks for the name twice, since neither can be undone.

## Working on the plugin

`--plugin-dir` cannot answer the configuration prompts, so the server will not start that
way. Point Claude Code at the binary directly instead:

```bash
just plugin-check                     # validate both manifests
claude --mcp-config mcp.json --strict-mcp-config
```

with an `mcp.json` naming the binary and the three `MXROUTE_*` variables.

[releases]: https://github.com/sshine/mxroute-rs/releases
[the panel]: https://panel.mxroute.com/api-keys.php
