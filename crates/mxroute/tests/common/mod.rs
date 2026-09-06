//! Shared harness for the mocked integration tests.
#![allow(clippy::expect_used, dead_code)]

use mxroute::{Client, Credentials, RateLimits};
use serde_json::{Value, json};
use wiremock::MockServer;
use wiremock::matchers::header;

/// The credentials every mocked test authenticates with.
pub const SERVER: &str = "eagle.mxlogin.com";
pub const USERNAME: &str = "johndoe";
pub const API_KEY: &str = "Mx8d989005f0cded8371b7d7271c50K1";

/// A mock server and a client pointed at it.
///
/// The client is built with no local rate limits and no retries, so a test never sleeps:
/// pacing and backoff have their own tests against a paused clock, and paying for them
/// here would only make the suite slow.
pub async fn mock() -> (MockServer, Client) {
    let server = MockServer::start().await;
    let client = client_for(&server);
    (server, client)
}

/// A client pointed at `server`, configured the way [`mock`] configures one.
pub fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .credentials(Credentials::new(SERVER, USERNAME, API_KEY))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(0)
        .build()
        .expect("valid configuration")
}

/// Matchers asserting all three credential headers are present and correct.
///
/// Applied to every mock, so a request that forgot one does not match and the test fails
/// with an unmatched request rather than a passing assertion.
pub fn auth() -> [wiremock::matchers::HeaderExactMatcher; 3] {
    [
        header("x-server", SERVER),
        header("x-username", USERNAME),
        header("x-api-key", API_KEY),
    ]
}

/// A success envelope around `data`, the shape almost every endpoint answers with.
pub fn ok(data: Value) -> Value {
    json!({ "success": true, "data": data })
}

/// An error document.
pub fn err(code: &str, message: &str) -> Value {
    json!({ "success": false, "error": { "code": code, "message": message } })
}

/// An error document naming the field that was rejected.
pub fn err_field(code: &str, message: &str, field: &str) -> Value {
    json!({
        "success": false,
        "error": { "code": code, "message": message, "field": field },
    })
}

/// A mailbox as the API reports one.
pub fn email_account_json(username: &str, quota: u32) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@example.com"),
        "quota": quota,
        "usage": 256.5,
        "limit": 9600,
        "sent": 42,
        "suspended": false,
    })
}

/// A domain as the API reports one.
pub fn domain_json(domain: &str) -> Value {
    json!({
        "domain": domain,
        "mail_hosting": true,
        "ssl_enabled": true,
        "pointers": [],
    })
}
