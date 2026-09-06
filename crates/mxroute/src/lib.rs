//! An async client for the [MXroute] email hosting API, covering the whole documented
//! surface: domains and their pointers, mailboxes, forwarders, spam filtering, catch-all
//! handling, DNS and quota reporting, and reseller users and packages.
//!
//! [MXroute]: https://mxroute.com
//!
//! # Getting started
//!
//! The API is a REST facade over DirectAdmin, so a client authenticates against one mail
//! server at a time and every request carries three headers rather than a single token.
//! All three are on the API Keys page at <https://panel.mxroute.com/api-keys.php>.
//!
//! ```no_run
//! use mxroute::api::email_accounts::NewEmailAccount;
//! use mxroute::{Client, Credentials, MailboxQuota};
//!
//! # async fn run() -> Result<(), mxroute::Error> {
//! let client = Client::new(Credentials::new(
//!     "eagle.mxlogin.com",
//!     "johndoe",
//!     "Mx8d989005f0cded8371b7d7271c50K1",
//! ))?;
//!
//! for domain in client.domains().list().await? {
//!     println!("{domain}");
//! }
//!
//! client
//!     .email_accounts("example.com")
//!     .create(
//!         &NewEmailAccount::new("sales", "Hunter2Hunter2")
//!             .quota(MailboxQuota::Megabytes(2048)),
//!     )
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! A [`Client`] is cheap to clone; clones share one connection pool, one rate-limiter
//! state and one request count. Use [`Client::with_credentials`] to address another mail
//! server while keeping all three.
//!
//! # Rate limiting
//!
//! MXroute throttles reads at 100 requests a minute and writes at 20. The client enforces
//! both itself, so it paces requests rather than collecting `429`s, and a `429` that does
//! arrive is honoured via `Retry-After` and retried.
//!
//! It also reads the `X-RateLimit-*` headers on every response. The local windows only
//! count what this client sent, so when the mxpanel or another process shares the account,
//! the server's remaining count is the only evidence that the allowance is already spent.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use mxroute::{Client, Credentials, Rate, RateLimits, Scope};
//!
//! # fn run(credentials: Credentials) -> Result<(), mxroute::Error> {
//! let client = Client::builder()
//!     .credentials(credentials)
//!     // Halve the write rate, because something else shares this account.
//!     .rate_limits(RateLimits::mxroute_defaults().with_scope(
//!         Scope::Write,
//!         [Rate::new(10, Duration::from_secs(60))?],
//!     ))
//!     .max_rate_limit_wait(Duration::from_secs(90))
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! Pass [`RateLimits::unlimited`] to stop predicting the documented rates. Waits the
//! *server* has reported still apply: opting out of pacing is not the same as ignoring a
//! throttle that already happened.
//!
//! [`Client::requests_made`] counts attempts rather than calls, so a retry and a throttle
//! replay are each visible where a caller counting its own calls would see one.
//!
//! # Errors
//!
//! [`Error`] is a `thiserror` enum. A rejected request keeps the server's document as an
//! [`ApiError`]: a typed [`ErrorCode`], the message, and for a validation failure the name
//! of the field that was rejected.
//!
//! ```
//! # use mxroute::{ApiError, ErrorCode};
//! let err = ApiError::parse(
//!     r#"{"success": false, "error": {"code": "VALIDATION_ERROR",
//!         "message": "Password too weak", "field": "password"}}"#,
//! );
//! assert_eq!(err.code, ErrorCode::Validation);
//! assert_eq!(err.field.as_deref(), Some("password"));
//! ```
//!
//! A code this crate has not been taught is kept as sent, and a body that is not an error
//! document at all keeps its text, so neither turns a reportable status into a decode
//! failure. The predicates — [`Error::is_not_found`], [`Error::is_conflict`],
//! [`Error::is_validation`] and the rest — are what most callers want.
//!
//! # Tracing
//!
//! Every request runs in an `mxroute.request` span carrying the method and path, with
//! events for the response status, local rate-limit waits, server throttling and retries.
//! Credentials are never recorded: [`Secret`] redacts itself in `Debug` and `Display`.
//!
//! # Types enforce API semantics
//!
//! Make illegal states unrepresentable, and make the wire form's surprises visible:
//!
//! - A mailbox quota of `0` means unlimited, so comparing the raw number gets that case
//!   backwards. [`MailboxQuota`] carries it, and orders unlimited above every finite size
//!   rather than below.
//! - `:blackhole:` and `:fail:` are forwarder destinations, not addresses. As strings they
//!   are indistinguishable from a typo, and mail discarded by a typo is not a failure
//!   anyone notices. [`Destination`] names them.
//! - A catch-all needs an address for exactly one of its three kinds.
//!   [`CatchAll`](api::catch_all::CatchAll) holds it inside that variant, so neither
//!   "address with no address" nor "fail with an address" is expressible in either
//!   direction.
//! - A domain pointer reads as a `type` of `alias` or `redirect` but writes as a boolean.
//!   [`PointerKind`](api::pointers::PointerKind) is what callers see both ways.
//! - Reseller packages read nested and typed, with `null` for unlimited, but write flat
//!   and stringly, with the literal `"unlimited"`.
//!   [`PackageQuota`](api::reseller::packages::PackageQuota) and
//!   [`PackageLimit`](api::reseller::packages::PackageLimit) bridge the two.
//! - A spam list entry travels in the URL path when it is removed, and the API's own
//!   character set permits `.` and `..`, which `url` folds away. [`SpamEntry`] rejects
//!   those, so no removal can address the list instead of an entry.
//! - [`SendLimit`] and [`SpamScore`] refuse values outside their documented ranges on the
//!   way out, but take whatever the server reports on the way in, so a change upstream
//!   cannot make existing data undecodable.
//!
//! # Caveats
//!
//! Three things about this API can lose data or surprise a caller, and none of them are
//! visible in a response:
//!
//! - **Spam sender lists are stored per account, not per domain.** They are reached
//!   through a domain, but whitelisting `*@trusted.com` under one domain whitelists it for
//!   every domain on the account, and removing an entry removes it everywhere.
//! - **Spam writes are not safely repeatable.** They are serialized against the mxpanel,
//!   so a `503` ([`Error::is_busy`]) means the lock was held and a `500` means the result
//!   could not be confirmed rather than that nothing happened. This client never replays a
//!   `PATCH`. Read the settings back and decide from what is stored.
//! - **Creating a forwarder to Gmail, Yahoo or AOL turns on Expert Spam Filtering for the
//!   whole domain**, affecting every other address in it.
//!
//! Also worth knowing:
//!
//! - `GET /domains` and the two reseller list endpoints answer with names, not objects, so
//!   reading every item's settings costs a request each.
//! - Adding a domain is not one call: fetch the key from
//!   [`verification_key`](api::AccountApi::verification_key), publish the TXT record, wait
//!   for DNS, then create the domain. Nothing reports whether propagation has finished.
//! - Quota figures are recomputed hourly, so a mailbox emptied a minute ago still shows
//!   its old size.
//! - `/quota` and `/quota/email` are the only endpoints that answer without the
//!   `{"success": …, "data": …}` envelope.
//!
//! # Not covered
//!
//! - DKIM management. The API exposes a domain's DKIM record read-only through
//!   [`DnsApi`](api::DnsApi) and offers no way to create or rotate one.
//! - Anything the mxpanel does that the REST API does not, such as creating an API key.

#![warn(missing_docs)]

pub mod api;
mod client;
mod error;
mod ratelimit;
mod types;

pub use client::{
    Client, ClientBuilder, Credentials, DEFAULT_BASE_URL, DEFAULT_USER_AGENT, MAX_RETRY_AFTER,
    Secret,
};
pub use error::{ApiError, Error, ErrorCode, InvalidValue, Result};
pub use ratelimit::{Rate, RateLimits, Scope};
pub use types::{
    Destination, MAX_SEND_LIMIT, MAX_SPAM_ENTRY_LEN, MailboxQuota, SendLimit, SpamEntry, SpamScore,
};
