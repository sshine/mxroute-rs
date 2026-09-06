//! Spam settings and the account-wide sender lists.
#![allow(clippy::expect_used)]

mod common;

use common::{mock, ok};
use mxroute::{SpamEntry, SpamScore};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn reading_the_settings_yields_the_threshold() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/spam/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({ "high_score": 7 }))))
        .expect(1)
        .mount(&server)
        .await;

    let settings = client
        .spam("example.com")
        .settings()
        .await
        .expect("the call succeeds");
    assert_eq!(settings.high_score, SpamScore::new(7).expect("in range"));
}

#[tokio::test]
async fn updating_the_threshold_sends_only_the_permitted_member() {
    let (server, client) = mock().await;
    // The API refuses unknown members, so an extra field would be a 400.
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/spam/settings"))
        .and(body_json(json!({ "high_score": 5 })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .spam("example.com")
        .set_high_score(SpamScore::new(5).expect("in range"))
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_threshold_outside_the_documented_range_never_reaches_the_wire() {
    assert!(SpamScore::new(0).is_err());
    assert!(SpamScore::new(51).is_err());
}

#[tokio::test]
async fn the_two_lists_are_separate_paths_under_the_same_domain() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/spam/whitelist"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(["*@trusted.com"]))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/spam/blacklist"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(["spammer@bad.example"]))))
        .expect(1)
        .mount(&server)
        .await;

    let spam = client.spam("example.com");
    let allowed = spam.whitelist().list().await.expect("the call succeeds");
    let blocked = spam.blacklist().list().await.expect("the call succeeds");
    assert_eq!(allowed[0].as_str(), "*@trusted.com");
    assert_eq!(blocked[0].as_str(), "spammer@bad.example");
}

#[tokio::test]
async fn adding_an_entry_sends_it_as_the_only_member() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/spam/whitelist"))
        .and(body_json(json!({ "entry": "*@trusted.com" })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let entry = SpamEntry::new("*@trusted.com").expect("a documented pattern");
    client
        .spam("example.com")
        .whitelist()
        .add(&entry)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn an_entry_already_present_is_a_conflict() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/spam/blacklist"))
        .respond_with(
            ResponseTemplate::new(409)
                .set_body_json(common::err("CONFLICT", "Entry already exists")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let entry = SpamEntry::new("spammer@bad.example").expect("a valid entry");
    let err = client
        .spam("example.com")
        .blacklist()
        .add(&entry)
        .await
        .expect_err("the entry is there already");
    assert!(err.is_conflict());
}

#[tokio::test]
async fn removing_a_plain_entry_addresses_it_in_the_path() {
    let (server, client) = mock().await;
    Mock::given(method("DELETE"))
        .and(path(
            "/domains/example.com/spam/blacklist/spammer@bad.example",
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let entry = SpamEntry::new("spammer@bad.example").expect("a valid entry");
    client
        .spam("example.com")
        .blacklist()
        .remove(&entry)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn an_entry_that_could_address_the_list_itself_is_refused_by_its_type() {
    // Both match the API's documented character set, and `url` folds both away, so a
    // deletion would address the collection. There is nothing to mock: the type refuses.
    for entry in [".", ".."] {
        assert!(SpamEntry::new(entry).is_err(), "{entry:?}");
    }
}

#[tokio::test]
async fn a_failed_update_leaves_the_caller_able_to_read_back_what_is_stored() {
    let (server, client) = mock().await;
    // The 500 here means "could not confirm", not "nothing happened", so the documented
    // recovery is a read rather than a retry. This is that sequence.
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/spam/settings"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(common::err("SERVER_ERROR", "unconfirmed")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/spam/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({ "high_score": 5 }))))
        .expect(1)
        .mount(&server)
        .await;

    let spam = client.spam("example.com");
    let score = SpamScore::new(5).expect("in range");
    spam.set_high_score(score)
        .await
        .expect_err("the write was not confirmed");

    // The write had in fact landed, which only a read can establish.
    let settings = spam.settings().await.expect("the read succeeds");
    assert_eq!(settings.high_score, score);
    // Two requests: the write was never replayed on the client's own initiative.
    assert_eq!(client.requests_made(), 2);
}
