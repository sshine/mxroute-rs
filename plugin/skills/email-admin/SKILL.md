---
name: email-admin
description: >-
  Administers MXroute email hosting through the mxroute MCP tools. Use when the user
  mentions "MXroute" or "mxroute", or asks to "add a mailbox", "create an email account",
  "set up email for a domain", "add a forwarder", "forward an address", "set a catch-all",
  "check my mailbox quota", "how much email storage is left", "mail is not being
  delivered", "check my SPF DKIM DMARC records", "blocklist this sender", "whitelist this
  address", "suspend a reseller user", or otherwise manages domains, mailboxes,
  forwarders, spam filtering or reseller accounts on a DirectAdmin or MXroute mail server.
allowed-tools:
  - mcp__plugin_mxroute_mxroute__mxroute_list_domains
  - mcp__plugin_mxroute_mxroute__mxroute_get_domain
  - mcp__plugin_mxroute_mxroute__mxroute_get_dns_records
  - mcp__plugin_mxroute_mxroute__mxroute_get_verification_key
  - mcp__plugin_mxroute_mxroute__mxroute_list_mailboxes
  - mcp__plugin_mxroute_mxroute__mxroute_list_forwarders
  - mcp__plugin_mxroute_mxroute__mxroute_get_spam_settings
  - mcp__plugin_mxroute_mxroute__mxroute_get_quota
license: MIT OR Apache-2.0
---

# Administering MXroute

Tool names below are given unqualified. In a session they carry the plugin's prefix, so
`mxroute_get_domain` is `mcp__plugin_mxroute_mxroute__mxroute_get_domain`.

Only the tools that read are pre-approved. Everything that creates, changes or deletes
prompts, which is deliberate: several of these calls cannot be undone.

## Before starting

- One account, one server. Everything is scoped to whatever `MXROUTE_SERVER` names.
- Domains, mailboxes, forwarders and pointers belong to a domain. **Spam sender lists and
  quota are account-wide**: an entry added through one domain applies to all of them.
- The server paces itself against 200 reads and 20 writes a minute, so a run of writes is
  slow rather than failing. Issue them one at a time.
- Quota figures are recomputed hourly. A mailbox emptied ten minutes ago still reports its
  old size, and that is not a bug to chase.
- The write tools appear only when the plugin's **Allow changes** option is on, and the
  reseller tools only with **Reseller account**. A missing tool usually means the option is
  off, not that the operation is impossible.

## If the mxroute tools are not there

`mxroute-mcp` is a separate binary that the plugin does not install. Suggest:

```
nix profile install github:sshine/mxroute-rs#mxroute-mcp
```

or an archive from <https://github.com/sshine/mxroute-rs/releases> placed on `PATH`. Then
restart Claude Code. `/plugin` reports the startup error in its Errors tab.

## Setting up a domain

1. `mxroute_get_verification_key` — the account-wide TXT record. It has to resolve *before*
   the domain is added, so fetch it first.
2. Give the user the record and wait for them to confirm it is published. Nothing in the API
   reports DNS propagation.
3. `mxroute_create_domain`.
4. `mxroute_get_dns_records` — hand over the MX, SPF and DKIM records exactly as given.
   Check them against live DNS with `dig`; the panel reports what it expects, not what
   resolves.
5. `mxroute_set_mail_hosting` with `enabled: true`.
6. `mxroute_create_mailbox` per address. Ask the user for each password or offer to generate
   one, and show it once. Do not invent a password silently.

A quota of `0` means unlimited. Omitting `quota_mb` takes the API's default, which is not
the same thing.

## Mail is not being delivered

Work outward from the account, since the cheap checks rule out the common causes:

1. `mxroute_get_domain` — is mail hosting even on? Check the catch-all in the same answer.
2. `mxroute_get_dns_records`, then `dig MX` and `dig TXT` for SPF and DKIM against live DNS.
   A mismatch here explains most delivery failures.
3. `mxroute_get_verification_key` — an unverified domain drops mail quietly.
4. `mxroute_get_quota` — a full mailbox bounces. Compare `over_quota` and the per-mailbox
   sizes, remembering the hourly lag.
5. `mxroute_list_mailboxes` — check `suspended`, and `sent` against `limit` for a sender who
   has hit the daily cap.
6. `mxroute_get_spam_settings` — is the sender blacklisted, or the threshold low enough to
   be catching real mail? See `references/deliverability.md`.

## Auditing storage

`mxroute_get_quota` answers with the account total, a breakdown by category and every
mailbox's size at once. `over_quota` and `limit_bytes` are computed for you; `limit_bytes`
is null when there is no limit. Report the largest mailboxes rather than the whole list.

## Spam lists

`mxroute_get_spam_settings` takes an `include` selector, so ask for only the sections
needed; each one costs a request.

Changes go through `mxroute_add_spam_sender` and `mxroute_remove_spam_sender` with
`list: "whitelist"` or `"blacklist"`. Say plainly that the change affects every domain on
the account before making it.

If one of these fails with the panel lock held, the write may already have applied. Read the
list back before trying again rather than repeating the call.

`mxroute_set_spam_score` takes 1 to 50. Lower catches more spam and more legitimate mail
with it; see `references/limits.md` before suggesting a number.

## Catch-all

`mxroute_set_catch_all` takes `fail`, `blackhole`, or `address` with an address. A catch-all
that accepts everything also accepts every address a spammer guesses, which is worth saying
before setting one. `fail` is the quiet default most domains want.

## Reseller users and packages

`mxroute_list_reseller_users` and `mxroute_list_reseller_packages` each take an optional
name to fetch one instead of the list.

A user's quota is in **megabytes**; a package's is in **gigabytes**. Nothing in the argument
names distinguishes them, so confirm the unit when a number sounds off by a thousand.

To stop someone's access without losing their mail, use
`mxroute_set_reseller_user_suspended`, not `mxroute_delete_reseller_user`. Deleting takes
their domains and stored mail with it.

Deleting a package leaves its users without one; move them with
`mxroute_set_reseller_user_package` first.

## Before anything destructive

`mxroute_delete_domain` and `mxroute_delete_reseller_user` ask for the name twice, and
neither can be undone. Repeat back exactly what is about to be removed and what it takes
with it, and wait for the user to agree. The same care applies to `mxroute_delete_mailbox`,
which destroys the stored mail.
