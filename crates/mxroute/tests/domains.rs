//! Domains, pointers, DNS info, catch-all and the verification key.
//!
//! These assert the path each call addresses and the body it sends. What happens to the
//! response — the envelope, error mapping, retries — is covered once in `transport.rs`.
#![allow(clippy::expect_used)]

mod common;

use common::{domain_json, mock, ok};
use mxroute::api::catch_all::CatchAll;
use mxroute::api::pointers::PointerKind;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn listing_domains_yields_bare_names() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(ok(json!(["example.com", "other.org"]))),
        )
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        client.domains().list().await.expect("the call succeeds"),
        vec!["example.com", "other.org"]
    );
}

#[tokio::test]
async fn creating_a_domain_sends_the_name_and_reads_back_the_narrower_response() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains"))
        .and(body_json(json!({ "domain": "example.com" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(ok(json!({
            "domain": "example.com",
            "ssl_enabled": false,
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let created = client
        .domains()
        .create("example.com")
        .await
        .expect("the call succeeds");
    assert_eq!(created.domain, "example.com");
    assert!(!created.ssl_enabled);
}

#[tokio::test]
async fn a_domain_reports_its_pointers_as_names() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "domain": "example.com",
            "mail_hosting": true,
            "ssl_enabled": true,
            "pointers": ["alias.com"],
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let domain = client
        .domains()
        .get("example.com")
        .await
        .expect("the call succeeds");
    assert_eq!(domain.pointers, vec!["alias.com"]);
}

#[tokio::test]
async fn a_domain_name_that_url_would_collapse_is_refused_before_a_request() {
    let (_server, client) = mock().await;
    // No mock is mounted: a request would fail the test by going unmatched.
    for name in ["", ".", ".."] {
        assert!(
            client
                .domains()
                .get(name)
                .await
                .expect_err("nothing should be sent")
                .is_validation()
        );
    }
    assert_eq!(client.requests_made(), 0);
}

#[tokio::test]
async fn enabling_mail_hosting_sends_the_flag() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/mail-status"))
        .and(body_json(json!({ "enabled": true })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .domains()
        .set_mail_hosting("example.com", true)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_redirect_pointer_writes_the_alias_flag_as_false() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/pointers"))
        .and(body_json(json!({ "pointer": "alias.com", "alias": false })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    client
        .pointers("example.com")
        .create("alias.com", PointerKind::Redirect)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn listing_pointers_reads_their_kind_from_the_type_field() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/pointers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!([
            { "pointer": "alias.com", "type": "alias", "target": "example.com" },
            { "pointer": "old.com", "type": "redirect", "target": "example.com" },
        ]))))
        .expect(1)
        .mount(&server)
        .await;

    let pointers = client
        .pointers("example.com")
        .list()
        .await
        .expect("the call succeeds");
    assert_eq!(pointers[0].kind, PointerKind::Alias);
    assert_eq!(pointers[1].kind, PointerKind::Redirect);
}

#[tokio::test]
async fn deleting_a_pointer_addresses_it_by_name() {
    let (server, client) = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/domains/example.com/pointers/alias.com"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client
        .pointers("example.com")
        .delete("alias.com")
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn dns_info_reports_the_records_to_publish() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/dns"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "mx_records": [
                { "priority": 10, "hostname": "eagle.mxlogin.com", "description": "Primary" },
            ],
            "spf": { "type": "TXT", "name": "@", "value": "v=spf1 include:mxroute.com -all" },
            "dkim": { "type": "TXT", "name": "x._domainkey", "value": "v=DKIM1; k=rsa; p=MII" },
            "verification": null,
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let info = client
        .dns("example.com")
        .get()
        .await
        .expect("the call succeeds");
    assert_eq!(info.mx_records[0].hostname, "eagle.mxlogin.com");
    assert_eq!(info.spf.name, "@");
    assert!(info.dkim.is_some());
    // Absent once the domain is verified.
    assert!(info.verification.is_none());
}

#[tokio::test]
async fn setting_a_catch_all_address_sends_both_members() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/catch-all"))
        .and(body_json(
            json!({ "type": "address", "address": "me@example.com" }),
        ))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .catch_all("example.com")
        .set(&CatchAll::Address("me@example.com".to_owned()))
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn setting_a_catch_all_to_fail_sends_no_address() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/catch-all"))
        .and(body_json(json!({ "type": "fail" })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .catch_all("example.com")
        .set(&CatchAll::Fail)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn reading_a_catch_all_yields_its_setting_and_description() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/catch-all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "type": "blackhole",
            "address": null,
            "description": "Discarded silently",
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let setting = client
        .catch_all("example.com")
        .get()
        .await
        .expect("the call succeeds");
    assert_eq!(setting.catch_all, CatchAll::Blackhole);
    assert_eq!(setting.description, "Discarded silently");
}

#[tokio::test]
async fn the_verification_key_comes_with_the_record_to_publish() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/verification-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "key": "_da-verify-a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
            "record": {
                "type": "TXT",
                "name": "_da-verify-a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
                "value": "domain-verified",
            },
            "description": "Add this TXT record before adding the domain",
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let key = client
        .account()
        .verification_key()
        .await
        .expect("the call succeeds");
    assert!(key.key.starts_with("_da-verify-"));
    assert_eq!(key.record.value, "domain-verified");
}

#[tokio::test]
async fn a_domain_handle_addresses_only_the_domain_it_was_built_for() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/first.com"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(domain_json("first.com"))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains/second.com"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(domain_json("second.com"))))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        client
            .domains()
            .get("first.com")
            .await
            .expect("succeeds")
            .domain,
        "first.com"
    );
    assert_eq!(
        client
            .domains()
            .get("second.com")
            .await
            .expect("succeeds")
            .domain,
        "second.com"
    );
}
