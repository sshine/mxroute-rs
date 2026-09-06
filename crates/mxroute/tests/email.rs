//! Mailboxes and forwarders.
#![allow(clippy::expect_used)]

mod common;

use common::{email_account_json, mock, ok};
use mxroute::api::email_accounts::{EmailAccountPatch, NewEmailAccount};
use mxroute::api::forwarders::NewForwarder;
use mxroute::{Destination, MailboxQuota, SendLimit};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn listing_mailboxes_decodes_their_quotas_and_usage() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/email-accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!([
            email_account_json("sales", 1024),
            email_account_json("support", 0),
        ]))))
        .expect(1)
        .mount(&server)
        .await;

    let accounts = client
        .email_accounts("example.com")
        .list()
        .await
        .expect("the call succeeds");
    assert_eq!(accounts[0].quota, MailboxQuota::Megabytes(1024));
    // The second has a zero quota, which is unlimited rather than nothing.
    assert_eq!(accounts[1].quota, MailboxQuota::Unlimited);
    assert_eq!(accounts[0].usage, 256.5);
    assert_eq!(accounts[0].email, "sales@example.com");
}

#[tokio::test]
async fn creating_a_mailbox_with_defaults_sends_only_what_was_asked_for() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/email-accounts"))
        .and(body_json(
            json!({ "username": "sales", "password": "Hunter2Hunter2" }),
        ))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    client
        .email_accounts("example.com")
        .create(&NewEmailAccount::new("sales", "Hunter2Hunter2"))
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn creating_a_mailbox_with_an_unlimited_quota_sends_a_zero() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/email-accounts"))
        .and(body_json(json!({
            "username": "archive",
            "password": "Hunter2Hunter2",
            "quota": 0,
            "limit": 100,
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let account = NewEmailAccount::new("archive", "Hunter2Hunter2")
        .quota(MailboxQuota::Unlimited)
        .send_limit(SendLimit::new(100).expect("under the cap"));
    client
        .email_accounts("example.com")
        .create(&account)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_patch_sends_only_the_fields_it_sets() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/email-accounts/sales"))
        .and(body_json(json!({ "quota": 2048 })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .email_accounts("example.com")
        .update(
            "sales",
            &EmailAccountPatch::new().quota(MailboxQuota::Megabytes(2048)),
        )
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_patch_that_would_change_nothing_is_refused_before_a_request() {
    let (_server, client) = mock().await;
    let err = client
        .email_accounts("example.com")
        .update("sales", &EmailAccountPatch::new())
        .await
        .expect_err("an empty body is not sent");
    assert!(err.is_validation());
    assert_eq!(client.requests_made(), 0);
}

#[tokio::test]
async fn a_send_limit_over_the_cap_never_reaches_the_wire() {
    // The type refuses it, so there is no request to mock.
    assert!(SendLimit::new(9601).is_err());
}

#[tokio::test]
async fn deleting_a_mailbox_addresses_it_by_local_part() {
    let (server, client) = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/domains/example.com/email-accounts/sales"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client
        .email_accounts("example.com")
        .delete("sales")
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_missing_mailbox_maps_onto_none() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/email-accounts/absent"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(common::err("NOT_FOUND", "No such account")),
        )
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        client
            .email_accounts("example.com")
            .try_get("absent")
            .await
            .expect("a 404 is not an error here"),
        None
    );
}

#[tokio::test]
async fn creating_a_forwarder_sends_its_destinations_as_strings() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/forwarders"))
        .and(body_json(json!({
            "alias": "sales",
            "destinations": ["someone@other.org", ":blackhole:"],
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let forwarder = NewForwarder::new(
        "sales",
        [
            Destination::address("someone@other.org"),
            Destination::Blackhole,
        ],
    )
    .expect("two destinations");
    client
        .forwarders("example.com")
        .create(&forwarder)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn listing_forwarders_recognizes_the_magic_destinations() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/forwarders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!([{
            "alias": "noreply",
            "email": "noreply@example.com",
            "destinations": [":fail:"],
        }]))))
        .expect(1)
        .mount(&server)
        .await;

    let forwarders = client
        .forwarders("example.com")
        .list()
        .await
        .expect("the call succeeds");
    assert_eq!(forwarders[0].destinations, vec![Destination::Fail]);
}

#[tokio::test]
async fn deleting_a_forwarder_addresses_it_by_alias() {
    let (server, client) = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/domains/example.com/forwarders/sales"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client
        .forwarders("example.com")
        .delete("sales")
        .await
        .expect("the call succeeds");
}
