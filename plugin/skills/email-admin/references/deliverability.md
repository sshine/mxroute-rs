# Why mail is not arriving

What each record does, and what its absence looks like from outside.

## The records

`mxroute_get_dns_records` reports what MXroute expects to find. Whether those records
actually resolve is a separate question, and the panel cannot answer it. Check with `dig`.

**MX** — where mail for the domain is delivered. Missing or pointing elsewhere, nothing
arrives at all and senders get a bounce naming the wrong host. Several records with
different priorities is normal.

**SPF** — a TXT record listing who may send as the domain. Missing, receivers treat the mail
as unauthenticated and many file it as spam. Two SPF records is worse than none: the check
fails outright rather than falling back.

**DKIM** — a TXT record holding the public key mail is signed with. Missing or stale, the
signature fails and receivers weight the mail down. This is the one that quietly breaks
after a domain is moved between hosts.

**DMARC** — not reported by MXroute, but worth checking at `_dmarc.<domain>`. A policy of
`p=reject` with SPF or DKIM broken turns a soft delivery problem into a hard one: mail stops
entirely rather than landing in spam.

**Verification** — the account-wide TXT record from `mxroute_get_verification_key`. Until it
resolves, the domain is not fully live and mail is dropped without a bounce.

## Symptoms

**Nothing arrives, senders get bounces.** MX is wrong, or mail hosting is off. Check
`mxroute_get_domain` first — it is one request and rules out the cheapest cause.

**Nothing arrives, senders get nothing back.** The domain is unverified, or a catch-all is
set to `blackhole` and the address does not exist. Blackhole accepts the message and
discards it, so the sender sees a successful delivery.

**Mail lands in spam.** SPF or DKIM failing, or the local spam score set too low. Read the
receiving side's headers if you can get them; they name which check failed.

**One sender's mail vanishes.** Check the blacklist with `mxroute_get_spam_settings`.
Remember the list is account-wide, so it may have been added while working on a different
domain.

**Mail arrives but replies do not send.** The mailbox has hit its daily send limit, or it is
suspended. `mxroute_list_mailboxes` reports `sent`, `limit` and `suspended`.

**Mail stopped arriving for one mailbox only.** It is full. A full mailbox bounces while the
rest of the domain works. Quota figures lag by up to an hour, so a mailbox that looks fine
may have filled since.

## Forwarding

Forwarding to Gmail, Yahoo or AOL turns on Expert Spam Filtering for the whole domain, which
affects every other address in it. Say so before adding one.

Forwarded mail fails SPF at the destination unless the receiver honours SRS, which is why
forwarding to a big provider is a common cause of mail disappearing for an address that
worked yesterday.

A destination of `:blackhole:` discards silently and `:fail:` bounces. Both are easy to set
by accident and neither leaves a trace at the sending end.
