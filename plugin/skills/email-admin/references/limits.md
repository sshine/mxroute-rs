# Numbers and units

The values the tools accept, and what each one means at its edges.

## Mailbox quota

`quota_mb` on `mxroute_create_mailbox` and `mxroute_update_mailbox` is in megabytes.

**Zero means unlimited**, not zero bytes. Omitting the argument is different again: the API
applies its own default. So there are three distinct outcomes and only two of them look
alike in a diff.

A mailbox reports `usage` as a percentage of its quota, and `mxroute_get_quota` reports each
mailbox's `size_bytes`. Both are recomputed hourly.

## Send limit

`send_limit` is messages per day, from 0 to 9600. The tool rejects a higher number before
making a request.

A mailbox that has hit its limit stops sending until the counter resets, without any error
visible to the sender. `mxroute_list_mailboxes` reports `sent` against `limit`, which is the
only way to see it coming.

## Spam score

`score` on `mxroute_set_spam_score` is 1 to 50. Mail scoring at or above it is treated as
spam and **deleted**, not quarantined.

Lower is more aggressive. The useful range in practice is narrow: below about 5 legitimate
mail starts disappearing, and above about 15 very little is caught. Change it a point or two
at a time and check what happens to real mail before going further.

## Spam list entries

An entry is an address like `sender@example.net` or a bare domain like `example.net`, up to
254 characters. `.` and `..` are rejected, because either would make the removal path
address the list itself rather than an entry in it.

Both lists are stored per account. Adding through `a.example` also affects `b.example`, and
removing through either removes it everywhere.

## Rate limits

200 reads and 20 writes a minute. The client paces itself against both and honours the
server's own countdown, so hitting the limit makes a run slow rather than making it fail.

The pacing only counts this client's own traffic. Another tool working on the same account
at the same time can still push it into a throttle.

## Reseller quotas

A **user's** quota is in megabytes. A **package's** is in gigabytes. Each has an
`_unlimited` flag beside it that wins over the number when set.

A package's caps on domains, mailboxes, forwarders and pointers are counts, each with its
own `_unlimited` flag.

## Reseller usernames

One to ten characters, lowercase letters, digits and underscores. It becomes part of every
later URL for that user and cannot be changed afterwards.
