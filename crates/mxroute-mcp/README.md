# mxroute-mcp

An MCP server over the [MXroute](https://mxroute.com) API. It gives Claude Code, Claude
Desktop and any other MCP client typed tools for domains, mailboxes, forwarders, catch-all,
spam filtering, DNS records and quota, on top of the [`mxroute`](../mxroute) client library.

## Install

**Nix**

```bash
nix profile install github:sshine/mxroute-rs#mxroute-mcp
nix run github:sshine/mxroute-rs#mxroute-mcp -- --help
```

**A release archive**

Download the one for your platform from [releases], check it against `SHA256SUMS`, and put
`mxroute-mcp` on your `PATH`. There are builds for x86-64 and ARM Linux (static, so any
distribution works), Apple silicon macOS, and x86-64 Windows.

**From source**

```bash
cargo install --git https://github.com/sshine/mxroute-rs mxroute-mcp
```

It is not on crates.io.

## Configure

Three variables, the same ones the library's live tests use. Create the key at
<https://panel.mxroute.com/api-keys.php>.

| Variable | Meaning |
| --- | --- |
| `MXROUTE_SERVER` | The server the account lives on, such as `eagle.mxlogin.com` |
| `MXROUTE_USERNAME` | The DirectAdmin username from the panel |
| `MXROUTE_API_KEY` | The API key |

There is deliberately no `--api-key` flag: a command line is readable by every process on
the host and is kept in shell history. Use the variable, or `--api-key-file`.

The server refuses to start without all three, rather than serving tools that would each
fail with a 401. An empty value counts as unset.

### As a Claude Code plugin

The [plugin](../../plugin) prompts for all three on install and keeps the key in the OS
keychain:

```bash
claude plugin marketplace add sshine/mxroute-rs
claude plugin install mxroute@sshine-mxroute
```

### As a plain MCP server

```json
{
  "mcpServers": {
    "mxroute": {
      "command": "mxroute-mcp",
      "env": {
        "MXROUTE_SERVER": "eagle.mxlogin.com",
        "MXROUTE_USERNAME": "johndoe",
        "MXROUTE_API_KEY": "..."
      }
    }
  }
}
```

Note that `claude mcp add --env MXROUTE_API_KEY=...` writes the key in plaintext into
`~/.claude.json`. The plugin exists partly to avoid that.

## What is served

Reads only, unless told otherwise. `--allow-writes` adds the tools that change something and
`--reseller` adds the reseller tools; `MXROUTE_MCP_ALLOW_WRITES` and `MXROUTE_MCP_RESELLER`
do the same, for clients that configure only an environment.

| Mode | Tools |
| --- | --- |
| default | 8 |
| `--allow-writes` | 22 |
| `--allow-writes --reseller` | 32 |

A tool that was not asked for is not registered at all, so it cannot be called by name
either.

**Reads.** `mxroute_list_domains`, `mxroute_get_domain`, `mxroute_get_dns_records`,
`mxroute_get_verification_key`, `mxroute_list_mailboxes`, `mxroute_list_forwarders`,
`mxroute_get_spam_settings`, `mxroute_get_quota`.

**Writes.** Create, update and delete for domains, pointers, mailboxes and forwarders, plus
`mxroute_set_mail_hosting`, `mxroute_set_catch_all`, `mxroute_set_spam_score`, and adding
and removing spam senders. Deleting a domain requires naming it twice.

**Reseller.** Listing, creating, updating and deleting users and packages, plus suspending a
user and moving one between packages. Deleting a user requires naming them twice.

`mxroute-mcp --list-tools` prints the schemas for a given mode and needs no credentials.

## Other options

```
--base-url <URL>           Point at something other than the live API
--timeout <SECONDS>        Per-request timeout, default 30
--max-items <N>            Rows a listing returns before it truncates, default 100
--check                    Verify the credentials against the API and exit
--list-tools               Print the tool schemas as JSON and exit
```

Logging goes to stderr and is off by default; `MXROUTE_MCP_LOG=debug` turns it on. It has to
be stderr, because stdout carries the protocol. `MXROUTE_MCP_LOG` is read in preference to
`RUST_LOG` so that a filter inherited from the client's environment does not flood the log.

## Rate limits and retries

The client paces itself against MXroute's 200 reads and 20 writes a minute and honours the
server's own `Retry-After`, so a run of writes is slow rather than failing.

The spam endpoints are the exception: they are serialized against the mxpanel, and a 503
there means the result could not be confirmed rather than that nothing happened. This server
never replays those, and says so in the error, because a retry can apply the change twice.

## License

Dual MIT and Apache-2.0, like the rest of the workspace. Note that the binary links
[rmcp](https://github.com/modelcontextprotocol/rust-sdk), which is Apache-2.0 only, so a
distributed build is effectively Apache-2.0 even though these sources are dual-licensed.

[releases]: https://github.com/sshine/mxroute-rs/releases
