//! The tools that change something.

mod common;

use common::{Harness, auth, email_account_json, err, ok, serve};
use mxroute_mcp::server::Mode;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn writes() -> Mode {
    Mode {
        writes: true,
        reseller: false,
    }
}

/// Mount one authenticated route answering `body` with `status`.
async fn route(h: &Harness, verb: &str, route: &str, status: u16, body: Value) {
    let mut mock = Mock::given(method(verb)).and(path(route));
    for matcher in auth() {
        mock = mock.and(matcher);
    }
    mock.respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&h.http)
        .await;
}

#[tokio::test]
async fn a_created_mailbox_is_read_back_so_the_applied_quota_is_visible() {
    let h = serve(writes()).await;

    let mut create = Mock::given(method("POST"))
        .and(path("/domains/a.example/email-accounts"))
        .and(body_partial_json(
            json!({"username": "sales", "quota": 2048}),
        ));
    for matcher in auth() {
        create = create.and(matcher);
    }
    create
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(null))))
        .mount(&h.http)
        .await;

    route(
        &h,
        "GET",
        "/domains/a.example/email-accounts/sales",
        200,
        ok(email_account_json("sales", 2048)),
    )
    .await;

    let body = h
        .read(
            "mxroute_create_mailbox",
            json!({
                "domain": "a.example",
                "username": "sales",
                "password": "Hunter2Hunter2",
                "quota_mb": 2048,
            }),
        )
        .await;
    assert_eq!(body["username"], json!("sales"));
    assert_eq!(body["quota"], json!(2048));
}

#[tokio::test]
async fn a_quota_of_zero_reaches_the_api_as_unlimited() {
    let h = serve(writes()).await;

    let mut create = Mock::given(method("POST"))
        .and(path("/domains/a.example/email-accounts"))
        .and(body_partial_json(json!({"quota": 0})));
    for matcher in auth() {
        create = create.and(matcher);
    }
    create
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(null))))
        .expect(1)
        .mount(&h.http)
        .await;

    route(
        &h,
        "GET",
        "/domains/a.example/email-accounts/big",
        200,
        ok(email_account_json("big", 0)),
    )
    .await;

    h.read(
        "mxroute_create_mailbox",
        json!({
            "domain": "a.example",
            "username": "big",
            "password": "Hunter2Hunter2",
            "quota_mb": 0,
        }),
    )
    .await;
}

#[tokio::test]
async fn a_send_limit_over_the_cap_is_refused_without_a_request() {
    let h = serve(writes()).await;

    let text = h
        .error(
            "mxroute_create_mailbox",
            json!({
                "domain": "a.example",
                "username": "sales",
                "password": "Hunter2Hunter2",
                "send_limit": 100_000,
            }),
        )
        .await;
    assert!(
        text.contains("send_limit") || text.contains("limit"),
        "{text}"
    );
    assert_eq!(
        h.http.received_requests().await.unwrap_or_default().len(),
        0
    );
}

#[tokio::test]
async fn an_update_that_changes_nothing_is_refused_without_a_request() {
    let h = serve(writes()).await;

    let text = h
        .error(
            "mxroute_update_mailbox",
            json!({"domain": "a.example", "username": "sales"}),
        )
        .await;
    assert!(text.contains("at least one"), "{text}");
    assert_eq!(
        h.http.received_requests().await.unwrap_or_default().len(),
        0
    );
}

#[tokio::test]
async fn a_deleted_mailbox_is_confirmed_by_name() {
    let h = serve(writes()).await;
    route(
        &h,
        "DELETE",
        "/domains/a.example/email-accounts/sales",
        200,
        ok(json!(null)),
    )
    .await;

    let result = h
        .call(
            "mxroute_delete_mailbox",
            json!({"domain": "a.example", "username": "sales"}),
        )
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert!(common::text(&result).contains("sales@a.example"));
}

#[tokio::test]
async fn a_catch_all_address_without_an_address_is_refused_without_a_request() {
    let h = serve(writes()).await;

    let text = h
        .error(
            "mxroute_set_catch_all",
            json!({"domain": "a.example", "mode": "address"}),
        )
        .await;
    assert!(text.contains("needs an address"), "{text}");
    assert_eq!(
        h.http.received_requests().await.unwrap_or_default().len(),
        0
    );
}

#[tokio::test]
async fn a_catch_all_address_is_sent_as_the_wire_shape_the_api_wants() {
    let h = serve(writes()).await;

    let mut set = Mock::given(method("PATCH"))
        .and(path("/domains/a.example/catch-all"))
        .and(body_partial_json(
            json!({"type": "address", "address": "in@a.example"}),
        ));
    for matcher in auth() {
        set = set.and(matcher);
    }
    set.respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(null))))
        .expect(1)
        .mount(&h.http)
        .await;

    let result = h
        .call(
            "mxroute_set_catch_all",
            json!({"domain": "a.example", "mode": "address", "address": "in@a.example"}),
        )
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
}

#[tokio::test]
async fn a_spam_sender_is_confirmed_as_account_wide_rather_than_per_domain() {
    // The one thing a caller is most likely to get wrong about these lists.
    let h = serve(writes()).await;
    route(
        &h,
        "POST",
        "/domains/a.example/spam/blacklist",
        200,
        ok(json!(null)),
    )
    .await;

    let result = h
        .call(
            "mxroute_add_spam_sender",
            json!({"domain": "a.example", "list": "blacklist", "sender": "spammer@example.net"}),
        )
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let text = common::text(&result);
    assert!(text.contains("every domain"), "{text}");
}

#[tokio::test]
async fn a_spam_entry_that_would_address_the_list_itself_is_refused() {
    // ".." as an entry would make the removal path point at the collection.
    let h = serve(writes()).await;

    let text = h
        .error(
            "mxroute_remove_spam_sender",
            json!({"domain": "a.example", "list": "blacklist", "sender": ".."}),
        )
        .await;
    assert!(!text.is_empty());
    assert_eq!(
        h.http.received_requests().await.unwrap_or_default().len(),
        0
    );
}

#[tokio::test]
async fn a_held_panel_lock_on_a_spam_write_says_the_write_may_have_applied() {
    let h = serve(writes()).await;
    route(
        &h,
        "POST",
        "/domains/a.example/spam/blacklist",
        503,
        err("SERVICE_UNAVAILABLE", "panel busy"),
    )
    .await;

    let text = h
        .error(
            "mxroute_add_spam_sender",
            json!({"domain": "a.example", "list": "blacklist", "sender": "spammer@example.net"}),
        )
        .await;
    assert!(text.contains("may or may not have applied"), "{text}");
    assert!(text.contains("does not replay"), "{text}");
}

#[tokio::test]
async fn a_domain_that_already_exists_points_at_the_tool_that_shows_it() {
    let h = serve(writes()).await;
    route(&h, "POST", "/domains", 409, err("CONFLICT", "exists")).await;

    let text = h
        .error("mxroute_create_domain", json!({"domain": "a.example"}))
        .await;
    assert!(text.contains("already"), "{text}");
    assert!(text.contains("mxroute_list_domains"), "{text}");
}
