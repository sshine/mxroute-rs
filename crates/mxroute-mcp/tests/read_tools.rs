//! The tools that read, and what they say when the API refuses.

mod common;

use common::{auth, domain_json, email_account_json, err, err_field, ok, serve};
use mxroute_mcp::server::Mode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

/// Mount one GET answering `body` with `status`.
async fn get(h: &common::Harness, route: &str, status: u16, body: serde_json::Value) {
    let mut mock = Mock::given(method("GET")).and(path(route));
    for matcher in auth() {
        mock = mock.and(matcher);
    }
    mock.respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&h.http)
        .await;
}

#[tokio::test]
async fn a_domain_listing_reports_how_many_it_returned() {
    let h = serve(Mode::default()).await;
    get(&h, "/domains", 200, ok(json!(["a.example", "b.example"]))).await;

    let body = h.read("mxroute_list_domains", json!({})).await;
    assert_eq!(body["total"], json!(2));
    assert_eq!(body["truncated"], json!(false));
    assert_eq!(body["items"], json!(["a.example", "b.example"]));
}

#[tokio::test]
async fn a_domain_carries_its_pointers_and_catch_all() {
    let h = serve(Mode::default()).await;
    get(&h, "/domains/a.example", 200, ok(domain_json("a.example"))).await;
    get(
        &h,
        "/domains/a.example/pointers",
        200,
        ok(json!([{ "pointer": "b.example", "type": "alias", "target": "a.example" }])),
    )
    .await;
    get(
        &h,
        "/domains/a.example/catch-all",
        200,
        ok(json!({ "type": "address", "address": "in@a.example", "description": "delivered" })),
    )
    .await;

    let body = h
        .read("mxroute_get_domain", json!({"domain": "a.example"}))
        .await;
    assert_eq!(body["domain"]["mail_hosting"], json!(true));
    assert_eq!(body["pointers"][0]["pointer"], json!("b.example"));
    assert_eq!(
        body["catch_all"]["catch_all"]["address"],
        json!("in@a.example")
    );
}

#[tokio::test]
async fn a_domain_whose_extras_fail_still_answers_with_the_domain() {
    // The catch-all endpoint can refuse for a domain that exists but has never had mail
    // turned on, which is exactly the domain someone would be looking at.
    let h = serve(Mode::default()).await;
    get(&h, "/domains/a.example", 200, ok(domain_json("a.example"))).await;
    get(&h, "/domains/a.example/pointers", 200, ok(json!([]))).await;
    get(
        &h,
        "/domains/a.example/catch-all",
        404,
        err("NOT_FOUND", "no catch-all"),
    )
    .await;

    let body = h
        .read("mxroute_get_domain", json!({"domain": "a.example"}))
        .await;
    assert_eq!(body["domain"]["domain"], json!("a.example"));
    assert!(body["catch_all"]["unavailable"].is_string(), "{body}");
}

#[tokio::test]
async fn one_named_mailbox_comes_back_alone() {
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/domains/a.example/email-accounts/sales",
        200,
        ok(email_account_json("sales", 2048)),
    )
    .await;

    let body = h
        .read(
            "mxroute_list_mailboxes",
            json!({"domain": "a.example", "username": "sales"}),
        )
        .await;
    assert_eq!(body["total"], json!(1));
    assert_eq!(body["items"][0]["username"], json!("sales"));
}

#[tokio::test]
async fn only_the_requested_spam_sections_are_fetched() {
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/domains/a.example/spam/blacklist",
        200,
        ok(json!(["spammer@example.net"])),
    )
    .await;

    let body = h
        .read(
            "mxroute_get_spam_settings",
            json!({"domain": "a.example", "include": ["blacklist"]}),
        )
        .await;
    assert_eq!(body["blacklist"], json!(["spammer@example.net"]));
    assert!(body.get("high_score").is_none(), "{body}");
    assert!(body.get("whitelist").is_none(), "{body}");
}

#[tokio::test]
async fn quota_answers_with_the_account_and_the_mailboxes_at_once() {
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/quota",
        200,
        ok(json!({
            "username": "johndoe",
            "total_used": 1024,
            "total_limit": 4096,
            "percent_used": 25.0,
            "breakdown": { "email": 1024, "web": 0, "databases": 0, "backups": 0, "other": 0 },
            "grace_period": null,
            "updated_at": "2026-01-01T00:00:00Z",
        })),
    )
    .await;
    get(
        &h,
        "/quota/email",
        200,
        ok(json!({
            "username": "johndoe",
            "accounts": [
                { "email_address": "sales@a.example", "size_bytes": 1024,
                  "updated_at": "2026-01-01T00:00:00Z" }
            ],
        })),
    )
    .await;

    let body = h.read("mxroute_get_quota", json!({})).await;
    assert_eq!(body["account"]["total_limit"], json!(4096));
    assert_eq!(body["over_quota"], json!(false));
    assert_eq!(body["email"]["accounts"][0]["size_bytes"], json!(1024));
}

// The error mapping. `mxroute::Error` is non-exhaustive and cannot be built from outside the
// library, so these drive it through the statuses that produce each variant.

#[tokio::test]
async fn a_missing_domain_points_at_the_tool_that_lists_them() {
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/domains/gone.example",
        404,
        err("NOT_FOUND", "no such domain"),
    )
    .await;

    let text = h
        .error("mxroute_get_domain", json!({"domain": "gone.example"}))
        .await;
    assert!(text.contains("no domain gone.example"), "{text}");
    assert!(text.contains("mxroute_list_domains"), "{text}");
}

#[tokio::test]
async fn a_rejected_key_names_the_variables_rather_than_the_call() {
    let h = serve(Mode::default()).await;
    get(&h, "/domains", 401, err("UNAUTHORIZED", "bad key")).await;

    let text = h.error("mxroute_list_domains", json!({})).await;
    assert!(text.contains("MXROUTE_API_KEY"), "{text}");
    assert!(text.contains("panel.mxroute.com"), "{text}");
}

#[tokio::test]
async fn a_rejected_value_names_the_field_the_api_blamed() {
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/domains/a.example/email-accounts/nope",
        422,
        err_field("VALIDATION_ERROR", "too long", "username"),
    )
    .await;

    let text = h
        .error(
            "mxroute_list_mailboxes",
            json!({"domain": "a.example", "username": "nope"}),
        )
        .await;
    assert!(text.contains("`username`"), "{text}");
    assert!(text.contains("too long"), "{text}");
}

#[tokio::test]
async fn a_held_panel_lock_says_to_read_back_rather_than_retry() {
    // Retrying is the wrong reflex here: the write may already have applied.
    let h = serve(Mode::default()).await;
    get(
        &h,
        "/domains/a.example/spam/blacklist",
        503,
        err("SERVICE_UNAVAILABLE", "panel busy"),
    )
    .await;

    let text = h
        .error(
            "mxroute_get_spam_settings",
            json!({"domain": "a.example", "include": ["blacklist"]}),
        )
        .await;
    assert!(text.contains("may or may not have applied"), "{text}");
    assert!(text.contains("Read the current state back"), "{text}");
}
