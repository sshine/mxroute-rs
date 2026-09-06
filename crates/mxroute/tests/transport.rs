//! Behaviour every endpoint shares: authentication, the response envelope, error mapping,
//! retries and throttling.
#![allow(clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{auth, client_for, err, err_field, mock, ok};
use mxroute::{Client, Credentials, ErrorCode, RateLimits};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mounts a mock, asserting the credential headers, that answers `template` once.
async fn mount(server: &MockServer, m: &str, p: &str, template: ResponseTemplate) {
    let mut mock = Mock::given(method(m)).and(path(p));
    for matcher in auth() {
        mock = mock.and(matcher);
    }
    mock.respond_with(template).expect(1).mount(server).await;
}

#[tokio::test]
async fn every_request_carries_the_three_credential_headers() {
    let (server, client) = mock().await;
    // The mock only matches when all three are right, so a missing one fails the call.
    mount(
        &server,
        "GET",
        "/domains",
        ResponseTemplate::new(200).set_body_json(ok(json!(["example.com"]))),
    )
    .await;

    let domains = client.domains().list().await.expect("the call succeeds");
    assert_eq!(domains, vec!["example.com"]);
}

#[tokio::test]
async fn the_success_envelope_is_unwrapped() {
    let (server, client) = mock().await;
    mount(
        &server,
        "GET",
        "/domains/example.com",
        ResponseTemplate::new(200).set_body_json(ok(common::domain_json("example.com"))),
    )
    .await;

    let domain = client
        .domains()
        .get("example.com")
        .await
        .expect("the call succeeds");
    assert_eq!(domain.domain, "example.com");
    assert!(domain.mail_hosting);
}

#[tokio::test]
async fn a_body_that_is_missing_its_envelope_fails_rather_than_being_guessed_at() {
    let (server, client) = mock().await;
    mount(
        &server,
        "GET",
        "/domains/example.com",
        // The domain object at the top level, with no `data` wrapper.
        ResponseTemplate::new(200).set_body_json(common::domain_json("example.com")),
    )
    .await;

    let err = client
        .domains()
        .get("example.com")
        .await
        .expect_err("the envelope is not optional");
    assert!(matches!(err, mxroute::Error::Decode { .. }), "{err:?}");
}

#[tokio::test]
async fn the_quota_endpoints_are_enveloped_like_everything_else() {
    let (server, client) = mock().await;
    // The OpenAPI document declares these two unenveloped. A live run says otherwise, and
    // this is the shape the server actually sends.
    mount(
        &server,
        "GET",
        "/quota",
        ResponseTemplate::new(200).set_body_json(ok(json!({
            "username": "johndoe",
            "total_used": 5_368_709_120u64,
            "total_limit": 10_737_418_240u64,
            "percent_used": 50.0,
            "updated_at": "2026-09-06T04:00:00Z",
        }))),
    )
    .await;

    let quota = client.quota().account().await.expect("the call succeeds");
    assert_eq!(quota.username, "johndoe");
    assert_eq!(quota.limit_bytes(), Some(10_737_418_240));
}

#[tokio::test]
async fn an_error_document_becomes_a_typed_error() {
    let (server, client) = mock().await;
    mount(
        &server,
        "POST",
        "/domains",
        ResponseTemplate::new(409).set_body_json(err("CONFLICT", "Domain already exists")),
    )
    .await;

    let err = client
        .domains()
        .create("example.com")
        .await
        .expect_err("the domain is taken");
    assert!(err.is_conflict());
    let api = err.api_error().expect("the body was an error document");
    assert_eq!(api.code, ErrorCode::Conflict);
    assert_eq!(api.message, "Domain already exists");
    assert_eq!(api.field, None);
}

#[tokio::test]
async fn a_validation_error_keeps_the_field_it_names() {
    let (server, client) = mock().await;
    mount(
        &server,
        "POST",
        "/domains/example.com/email-accounts",
        ResponseTemplate::new(400).set_body_json(err_field(
            "VALIDATION_ERROR",
            "Password too weak",
            "password",
        )),
    )
    .await;

    let account = mxroute::api::email_accounts::NewEmailAccount::new("sales", "short");
    let err = client
        .email_accounts("example.com")
        .create(&account)
        .await
        .expect_err("the password is rejected");
    assert!(err.is_validation());
    assert_eq!(
        err.api_error().and_then(|api| api.field.as_deref()),
        Some("password")
    );
}

#[tokio::test]
async fn an_error_body_that_is_not_json_still_reports_its_status() {
    let (server, client) = mock().await;
    mount(
        &server,
        "GET",
        "/domains",
        ResponseTemplate::new(502).set_body_string("<html>502 Bad Gateway</html>"),
    )
    .await;

    let err = client
        .domains()
        .list()
        .await
        .expect_err("the gateway failed");
    assert_eq!(err.status().map(|s| s.as_u16()), Some(502));
    let api = err.api_error().expect("there is still a body");
    assert_eq!(api.code, ErrorCode::Unparsed);
    assert!(api.message.contains("502 Bad Gateway"));
}

#[tokio::test]
async fn a_missing_resource_maps_onto_none_only_where_asked() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/domains/absent.com"))
        .respond_with(ResponseTemplate::new(404).set_body_json(err("NOT_FOUND", "No such domain")))
        .expect(2)
        .mount(&server)
        .await;

    assert_eq!(
        client
            .domains()
            .try_get("absent.com")
            .await
            .expect("a 404 is not an error here"),
        None
    );
    assert!(
        client
            .domains()
            .get("absent.com")
            .await
            .expect_err("get reports it")
            .is_not_found()
    );
}

#[tokio::test]
async fn a_204_with_no_body_is_a_success() {
    let (server, client) = mock().await;
    mount(
        &server,
        "DELETE",
        "/domains/example.com",
        ResponseTemplate::new(204),
    )
    .await;

    client
        .domains()
        .delete("example.com")
        .await
        .expect("a 204 carries no body to decode");
}

#[tokio::test]
async fn a_write_whose_response_body_is_undocumented_still_succeeds() {
    let (server, client) = mock().await;
    // The spec documents a 201 with no body for this one, but the API may answer with
    // anything; neither shape should fail the call.
    mount(
        &server,
        "POST",
        "/domains/example.com/email-accounts",
        ResponseTemplate::new(201).set_body_string("Account created"),
    )
    .await;

    let account = mxroute::api::email_accounts::NewEmailAccount::new("sales", "Hunter2Hunter2");
    client
        .email_accounts("example.com")
        .create(&account)
        .await
        .expect("an unexpected body is not a failure");
}

#[tokio::test]
async fn a_request_body_is_sent_as_json() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/mail-status"))
        .and(body_json(json!({ "enabled": false })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .domains()
        .set_mail_hosting("example.com", false)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn requests_made_counts_attempts_including_retries() {
    let server = MockServer::start().await;
    let client = Client::builder()
        .credentials(Credentials::new(
            common::SERVER,
            common::USERNAME,
            common::API_KEY,
        ))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(2)
        .max_retry_delay(Duration::from_millis(1))
        .build()
        .expect("valid configuration");

    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(ResponseTemplate::new(500).set_body_json(err("SERVER_ERROR", "boom")))
        .expect(3)
        .mount(&server)
        .await;

    assert_eq!(client.requests_made(), 0);
    client
        .domains()
        .list()
        .await
        .expect_err("every attempt failed");
    // The first attempt plus two retries.
    assert_eq!(client.requests_made(), 3);
}

#[tokio::test]
async fn a_server_error_is_retried_for_a_get() {
    let server = MockServer::start().await;
    let client = Client::builder()
        .credentials(Credentials::new(
            common::SERVER,
            common::USERNAME,
            common::API_KEY,
        ))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(1)
        .max_retry_delay(Duration::from_millis(1))
        .build()
        .expect("valid configuration");

    // Wiremock tries mocks in mount order, and the first stops matching once it has
    // answered once, so this pair is "fail once, then succeed".
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(ResponseTemplate::new(500).set_body_json(err("SERVER_ERROR", "boom")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(["example.com"]))))
        .mount(&server)
        .await;

    let domains = client.domains().list().await.expect("the retry succeeded");
    assert_eq!(domains, vec!["example.com"]);
    assert_eq!(client.requests_made(), 2);
}

#[tokio::test]
async fn a_server_error_is_not_retried_for_a_patch() {
    let server = MockServer::start().await;
    let client = Client::builder()
        .credentials(Credentials::new(
            common::SERVER,
            common::USERNAME,
            common::API_KEY,
        ))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(3)
        .max_retry_delay(Duration::from_millis(1))
        .build()
        .expect("valid configuration");

    // A spam settings write is the case this rule exists for: the update may have been
    // applied before the response was lost, so replaying it is not safe.
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/spam/settings"))
        .respond_with(ResponseTemplate::new(500).set_body_json(err("SERVER_ERROR", "unconfirmed")))
        .expect(1)
        .mount(&server)
        .await;

    let score = mxroute::SpamScore::new(5).expect("in range");
    client
        .spam("example.com")
        .set_high_score(score)
        .await
        .expect_err("the write failed");
    assert_eq!(client.requests_made(), 1, "a PATCH must not be replayed");
}

#[tokio::test]
async fn a_503_from_the_spam_lock_is_reported_as_busy_and_not_retried() {
    let (server, client) = mock().await;
    mount(
        &server,
        "PATCH",
        "/domains/example.com/spam/settings",
        ResponseTemplate::new(503).set_body_json(err("BUSINESS_ERROR", "Another update running")),
    )
    .await;

    let score = mxroute::SpamScore::new(5).expect("in range");
    let err = client
        .spam("example.com")
        .set_high_score(score)
        .await
        .expect_err("the lock was held");
    assert!(err.is_busy());
    assert_eq!(client.requests_made(), 1);
}

#[tokio::test]
async fn a_throttled_request_honours_retry_after_and_replays() {
    let server = MockServer::start().await;
    let client = Client::builder()
        .credentials(Credentials::new(
            common::SERVER,
            common::USERNAME,
            common::API_KEY,
        ))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(1)
        .build()
        .expect("valid configuration");

    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(err("RATE_LIMITED", "Too many requests")),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!([]))))
        .mount(&server)
        .await;

    let domains = client.domains().list().await.expect("the replay succeeded");
    assert!(domains.is_empty());
    assert_eq!(client.requests_made(), 2);
}

#[tokio::test]
async fn a_throttle_that_outlasts_the_retry_budget_reports_itself() {
    let (server, client) = mock().await;
    mount(
        &server,
        "GET",
        "/domains",
        ResponseTemplate::new(429)
            .insert_header("retry-after", "60")
            .set_body_json(err("RATE_LIMITED", "Too many requests")),
    )
    .await;

    let err = client
        .domains()
        .list()
        .await
        .expect_err("max_retries is zero");
    assert!(err.is_rate_limited());
    assert!(matches!(
        err,
        mxroute::Error::RateLimited {
            attempts: 1,
            retry_after: Some(_),
            ..
        }
    ));
}

#[tokio::test]
async fn an_exhausted_allowance_in_the_headers_holds_the_next_request_back() {
    let server = MockServer::start().await;
    // A wait longer than this fails rather than sleeping, which is how the test observes
    // that the header was acted on without waiting out a real minute.
    let client = Client::builder()
        .credentials(Credentials::new(
            common::SERVER,
            common::USERNAME,
            common::API_KEY,
        ))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(0)
        .max_rate_limit_wait(Duration::from_millis(1))
        .build()
        .expect("valid configuration");

    let reset = chrono::Utc::now().timestamp() + 300;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-limit", "100")
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str())
                .set_body_json(ok(json!([]))),
        )
        .expect(1)
        .mount(&server)
        .await;

    client.domains().list().await.expect("the first call goes");
    let err = client
        .domains()
        .list()
        .await
        .expect_err("the allowance was reported spent");
    assert!(err.is_rate_limited());
    assert!(matches!(err, mxroute::Error::RateLimitWouldBlock { .. }));
}

#[tokio::test]
async fn a_path_segment_is_encoded_rather_than_inventing_a_route() {
    let (server, client) = mock().await;
    let entry = mxroute::SpamEntry::new("*@trusted.com").expect("a documented pattern");
    mount(
        &server,
        "DELETE",
        "/domains/example.com/spam/whitelist/*@trusted.com",
        ResponseTemplate::new(204),
    )
    .await;

    client
        .spam("example.com")
        .whitelist()
        .remove(&entry)
        .await
        .expect("the wildcard survived as one segment");
}

#[tokio::test]
async fn a_client_derived_for_another_server_sends_the_new_credentials() {
    let server = MockServer::start().await;
    let first = client_for(&server);
    let second = first
        .with_credentials(Credentials::new("hawk.mxlogin.com", "janedoe", "otherkey"))
        .expect("valid credentials");

    Mock::given(method("GET"))
        .and(path("/domains"))
        .and(wiremock::matchers::header("x-server", "hawk.mxlogin.com"))
        .and(wiremock::matchers::header("x-username", "janedoe"))
        .and(wiremock::matchers::header("x-api-key", "otherkey"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!([]))))
        .expect(1)
        .mount(&server)
        .await;

    second.domains().list().await.expect("the call succeeds");
    // One counter across both, because they share it.
    assert_eq!(first.requests_made(), 1);
}
