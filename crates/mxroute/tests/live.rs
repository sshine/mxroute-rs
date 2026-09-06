//! Tests that talk to the real API.
//!
//! Every test here is `#[ignore]`d rather than feature-gated, so `--all-features` cannot
//! switch them on by accident. Run them with `just live-test`, which resolves the
//! credentials through secretspec.
//!
//! Most of these read. The ones that write are additionally gated on an environment
//! variable naming what they may operate on, because there is no sandbox: every call lands
//! on a real account.
//!
//! Several exist to settle questions the OpenAPI document cannot. Where the spec and the
//! server disagree, these are what say so, and each names the shape it expected.
#![allow(clippy::expect_used)]

use std::env;

use mxroute::{Client, Credentials, RateLimits, Scope, SpamEntry};

/// Builds a client from the environment, or explains what is missing.
///
/// `secretspec run` is what puts these there. A test that cannot find them fails rather
/// than passing quietly, since a silently skipped live suite is the same as none.
fn client() -> Client {
    fn var(name: &str) -> String {
        env::var(name).unwrap_or_else(|_| {
            panic!("{name} is not set; run the live suite with `just live-test`")
        })
    }

    Client::new(Credentials::new(
        var("MXROUTE_SERVER"),
        var("MXROUTE_USERNAME"),
        var("MXROUTE_API_KEY"),
    ))
    .expect("the credentials build a client")
}

/// The scratch domain the destructive tests operate on, or `None` to skip them.
fn scratch_domain() -> Option<String> {
    env::var("MXROUTE_TEST_DOMAIN")
        .ok()
        .filter(|d| !d.is_empty())
}

/// The scratch domain, checked to be on the account.
///
/// Naming a domain the account does not hold otherwise fails several tests deep with a
/// bare `404`, which reads like a client bug rather than an unfinished setup. Adding a
/// domain is a DNS round trip and a wait, so it cannot be done here.
async fn scratch_domain_on_account(client: &Client) -> Option<String> {
    let domain = scratch_domain()?;
    let held = client.domains().list().await.expect("the account lists");
    assert!(
        held.contains(&domain),
        "MXROUTE_TEST_DOMAIN is {domain:?}, which is not on this account (it holds {held:?}).\n\
         Add it first: publish the TXT record from `verification_key`, wait for DNS, then\n\
         create the domain. Or unset MXROUTE_TEST_DOMAIN to skip the tests that write."
    );
    Some(domain)
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn listing_domains_works() {
    let client = client();
    let domains = client.domains().list().await.expect("the account lists");
    println!("{} domain(s): {domains:?}", domains.len());
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn every_listed_domain_can_be_read_back() {
    let client = client();
    for name in client.domains().list().await.expect("the account lists") {
        let domain = client
            .domains()
            .get(&name)
            .await
            .expect("a listed domain reads back");
        assert_eq!(domain.domain, name);
        println!("{name}: mail_hosting={}", domain.mail_hosting);
    }
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_verification_key_has_the_documented_shape() {
    let client = client();
    match client.account().verification_key().await {
        Ok(key) => {
            assert!(
                key.key.starts_with("_da-verify-"),
                "key was {:?}, expected the documented _da-verify- prefix",
                key.key
            );
            assert_eq!(key.record.record_type, "TXT");
            println!("verification key: {}", key.key);
        }
        // The account may simply not have one, which the API reports as a 500.
        Err(err) if err.status().map(|s| s.as_u16()) == Some(500) => {
            println!("no verification key on this account: {err}");
        }
        Err(err) => panic!("unexpected failure: {err}"),
    }
}

/// The spec declares `/quota` with its fields at the top level, unlike every other
/// endpoint. A live run showed that is wrong — the server envelopes these too — so this
/// now guards the correction rather than the claim.
#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_quota_endpoints_are_enveloped_like_everything_else() {
    let client = client();
    let quota = client
        .quota()
        .account()
        .await
        .expect("/quota decodes from its data member");
    println!(
        "{}: {} bytes used of {:?}",
        quota.username,
        quota.total_used,
        quota.limit_bytes()
    );

    let email = client
        .quota()
        .email()
        .await
        .expect("/quota/email decodes from its data member");
    println!("{} mailbox(es) reported", email.accounts.len());
    // Documented as sorted largest first.
    let sizes: Vec<u64> = email.accounts.iter().map(|a| a.size_bytes).collect();
    let mut sorted = sizes.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(sizes, sorted, "mailboxes were not largest-first");
}

/// The `X-RateLimit-*` headers are documented as present on every response. The client
/// paces itself against them, so their absence would silently disable that. Checked with a
/// bare request, since the client does not surface response headers.
#[tokio::test]
#[ignore = "talks to the real API"]
async fn every_response_carries_the_rate_limit_headers() {
    let response = reqwest::Client::new()
        .get("https://api.mxroute.com/domains")
        .header("x-server", env::var("MXROUTE_SERVER").expect("set"))
        .header("x-username", env::var("MXROUTE_USERNAME").expect("set"))
        .header("x-api-key", env::var("MXROUTE_API_KEY").expect("set"))
        .send()
        .await
        .expect("the request goes through");

    for header in [
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ] {
        let value = response.headers().get(header);
        assert!(value.is_some(), "{header} was absent");
        println!("{header}: {value:?}");
    }

    // The documented read rate is 100 and the observed one is 200, so this crate paces
    // against the header rather than the documentation. That makes the header the thing
    // to notice changing, and nothing else would notice: pacing too slowly costs only
    // throughput, which no test measures.
    let reported: u32 = response
        .headers()
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .expect("the limit header is a number");
    let paced = RateLimits::mxroute_defaults().rates(Scope::Read)[0].limit();
    assert_eq!(
        reported, paced,
        "the server allows {reported} reads a minute but this client paces at {paced}; \
         update RateLimits::mxroute_defaults"
    );
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn the_scratch_domains_settings_read_back() {
    let client = client();
    let Some(domain) = scratch_domain_on_account(&client).await else {
        println!("MXROUTE_TEST_DOMAIN is unset; skipping");
        return;
    };

    let dns = client.dns(&domain).get().await.expect("DNS info reads");
    assert!(!dns.mx_records.is_empty(), "no MX records reported");
    println!("{domain}: {} MX record(s)", dns.mx_records.len());

    let catch_all = client
        .catch_all(&domain)
        .get()
        .await
        .expect("the catch-all reads");
    println!("{domain}: catch-all {:?}", catch_all.catch_all);

    let accounts = client
        .email_accounts(&domain)
        .list()
        .await
        .expect("mailboxes list");
    println!("{domain}: {} mailbox(es)", accounts.len());

    let forwarders = client
        .forwarders(&domain)
        .list()
        .await
        .expect("forwarders list");
    println!("{domain}: {} forwarder(s)", forwarders.len());
}

/// What `POST /domains/{d}/email-accounts` actually answers with, which the spec leaves
/// unspecified. The client treats it as bodyless; this confirms that is enough.
#[tokio::test]
#[ignore = "talks to the real API and creates a mailbox"]
async fn a_mailbox_can_be_created_read_and_deleted() {
    let client = client();
    let Some(domain) = scratch_domain_on_account(&client).await else {
        println!("MXROUTE_TEST_DOMAIN is unset; skipping");
        return;
    };
    let username = "mxroute-rs-livetest";
    let accounts = client.email_accounts(&domain);

    // Left behind by an earlier run that died before its sweep.
    if accounts
        .try_get(username)
        .await
        .expect("the read works")
        .is_some()
    {
        accounts.delete(username).await.expect("the sweep works");
    }

    let new = mxroute::api::email_accounts::NewEmailAccount::new(username, "Livetest1Password")
        .quota(mxroute::MailboxQuota::Megabytes(10));
    accounts
        .create(&new)
        .await
        .expect("creation succeeds with whatever body it answers with");

    let account = accounts
        .get(username)
        .await
        .expect("the mailbox reads back");
    assert_eq!(account.username, username);
    assert_eq!(account.quota, mxroute::MailboxQuota::Megabytes(10));

    accounts
        .update(
            username,
            &mxroute::api::email_accounts::EmailAccountPatch::new()
                .quota(mxroute::MailboxQuota::Megabytes(20)),
        )
        .await
        .expect("the update succeeds");
    assert_eq!(
        accounts.get(username).await.expect("reads back").quota,
        mxroute::MailboxQuota::Megabytes(20)
    );

    accounts.delete(username).await.expect("deletion succeeds");
    assert_eq!(
        accounts.try_get(username).await.expect("the read works"),
        None
    );
}

/// Gated separately because the sender lists are account-wide: this touches every domain
/// on the account, not just the scratch one.
#[tokio::test]
#[ignore = "talks to the real API and changes account-wide spam settings"]
async fn a_whitelist_entry_can_be_added_and_removed() {
    // Checked before anything is fetched: this gate is the cheap one, and it is the one
    // that stops a test from changing filtering for every domain on the account.
    if env::var("MXROUTE_TEST_SPAM").is_err() {
        println!("MXROUTE_TEST_SPAM is unset; skipping an account-wide change");
        return;
    }
    let client = client();
    let Some(domain) = scratch_domain_on_account(&client).await else {
        println!("MXROUTE_TEST_DOMAIN is unset; skipping");
        return;
    };
    let whitelist = client.spam(&domain).whitelist();
    let entry = SpamEntry::new("mxroute-rs-livetest@example.com").expect("a valid entry");

    let before = whitelist.list().await.expect("the list reads");
    if before.contains(&entry) {
        whitelist.remove(&entry).await.expect("the sweep works");
    }

    whitelist.add(&entry).await.expect("the addition succeeds");
    assert!(
        whitelist
            .list()
            .await
            .expect("the list reads")
            .contains(&entry),
        "the entry did not appear"
    );

    whitelist.remove(&entry).await.expect("removal succeeds");
    assert!(
        !whitelist
            .list()
            .await
            .expect("the list reads")
            .contains(&entry),
        "the entry did not go away"
    );
}

#[tokio::test]
#[ignore = "talks to the real API and needs a reseller account"]
async fn reseller_users_and_packages_list() {
    if env::var("MXROUTE_TEST_RESELLER").is_err() {
        println!("MXROUTE_TEST_RESELLER is unset; skipping");
        return;
    }
    let client = client();
    let reseller = client.reseller();

    let users = reseller.users().list().await.expect("users list");
    println!("{} reseller user(s)", users.len());

    for name in reseller.packages().list().await.expect("packages list") {
        let package = reseller
            .packages()
            .get(&name)
            .await
            .expect("a listed package reads back");
        println!("package {name}: quota {:?}", package.settings.quota());
    }
}

#[tokio::test]
#[ignore = "talks to the real API"]
async fn a_domain_that_is_not_ours_is_a_404_rather_than_a_403() {
    let client = client();
    let err = client
        .domains()
        .get("example.com")
        .await
        .expect_err("example.com is not on this account");
    // Documented behaviour: absent and not-yours are the same answer, which is why
    // is_not_found's docs say a 404 does not mean the name is free.
    assert!(
        err.is_not_found(),
        "expected 404, got {:?}: {err}",
        err.status()
    );
}

/// A domain under our control that is deliberately never added to the account.
///
/// Under `shine.town` rather than `example.com` so the negative case is one we could add
/// but have not, which is the state a caller is actually in before creating a domain.
/// Nothing needs to exist in DNS for this: the API answers from the account, not the zone.
const ABSENT_DOMAIN: &str = "mxtestnegative.shine.town";

fn absent_domain() -> String {
    env::var("MXROUTE_TEST_NEGATIVE_DOMAIN").unwrap_or_else(|_| ABSENT_DOMAIN.to_owned())
}

/// Every domain-scoped group, against a domain the account does not hold.
///
/// One test rather than nine, because each is a request and the read allowance is spent
/// across the whole suite. Nine reads is well inside a minute's budget; nine tests each
/// building a client and listing domains first would not be.
///
/// Guards two things the type system cannot: that a missing domain is reported the same
/// way whichever group is asked, and that `get` and `try_get` disagree only in how they
/// say so.
#[tokio::test]
#[ignore = "talks to the real API"]
async fn every_group_reports_an_absent_domain_the_same_way() {
    let client = client();
    let absent = absent_domain();

    // Fail early and loudly if the account has grown the domain this test assumes it
    // lacks, rather than reporting a pile of confusing assertion failures below.
    let held = client.domains().list().await.expect("the account lists");
    assert!(
        !held.contains(&absent),
        "{absent} is on this account, so it cannot serve as the negative case. \
         Point MXROUTE_TEST_NEGATIVE_DOMAIN at something absent."
    );

    let err = client
        .domains()
        .get(&absent)
        .await
        .expect_err("the domain is not on the account");
    assert!(err.is_not_found(), "domains().get: {err}");

    macro_rules! assert_not_found {
        ($label:literal, $call:expr) => {
            let err = $call
                .await
                .expect_err(concat!($label, " should not be found"));
            assert!(err.is_not_found(), concat!($label, ": {}"), err);
        };
    }

    assert_not_found!("dns", client.dns(&absent).get());
    assert_not_found!("email_accounts", client.email_accounts(&absent).list());
    assert_not_found!("forwarders", client.forwarders(&absent).list());
    assert_not_found!("pointers", client.pointers(&absent).list());
    assert_not_found!("catch_all", client.catch_all(&absent).get());
    assert_not_found!("spam", client.spam(&absent).settings());

    println!("{absent}: every group answered 404");
}

/// `try_get` exists so a caller can ask whether something is there without matching on an
/// error. This is the case it was added for.
#[tokio::test]
#[ignore = "talks to the real API"]
async fn try_get_maps_an_absent_domain_onto_none() {
    let client = client();
    let absent = absent_domain();

    assert_eq!(
        client
            .domains()
            .try_get(&absent)
            .await
            .expect("a 404 is not an error here"),
        None
    );

    // Same for a mailbox inside a domain that does not exist: the 404 is about the
    // domain, but it reaches the caller through the mailbox call.
    assert_eq!(
        client
            .email_accounts(&absent)
            .try_get("nobody")
            .await
            .expect("a 404 is not an error here"),
        None
    );
}
