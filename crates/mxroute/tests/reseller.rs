//! Reseller users and packages.
#![allow(clippy::expect_used)]

mod common;

use common::{mock, ok};
use mxroute::api::reseller::packages::{PackageLimit, PackageQuota, PackageSpec};
use mxroute::api::reseller::users::{NewResellerUser, ResellerUserPatch};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn user_json(username: &str) -> serde_json::Value {
    json!({
        "username": username,
        "email": format!("{username}@example.com"),
        "domain": "example.com",
        "package": "basic",
        "suspended": false,
        "quota": { "limit": 10240, "used": 512.5, "unlimited": false },
    })
}

#[tokio::test]
async fn listing_users_yields_bare_names() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/reseller/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!(["johndoe", "janedoe"]))))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        client
            .reseller()
            .users()
            .list()
            .await
            .expect("the call succeeds"),
        vec!["johndoe", "janedoe"]
    );
}

#[tokio::test]
async fn a_non_reseller_account_is_told_so() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/reseller/users"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(common::err("FORBIDDEN", "Requires reseller privileges")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client
        .reseller()
        .users()
        .list()
        .await
        .expect_err("not a reseller");
    assert!(err.is_forbidden());
}

#[tokio::test]
async fn creating_a_user_sends_all_four_required_members() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/reseller/users"))
        .and(body_json(json!({
            "username": "johndoe",
            "email": "john@example.com",
            "password": "Hunter2Hunter2",
            "package": "basic",
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let user = NewResellerUser::new("johndoe", "john@example.com", "Hunter2Hunter2", "basic")
        .expect("a valid username");
    client
        .reseller()
        .users()
        .create(&user)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn a_users_quota_patch_sends_megabytes_as_a_string() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/reseller/users/johndoe"))
        .and(body_json(json!({ "quota": "2048" })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    client
        .reseller()
        .users()
        .update("johndoe", &ResellerUserPatch::new().quota_megabytes(2048))
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn suspending_is_an_action_rather_than_a_field() {
    let (server, client) = mock().await;
    Mock::given(method("POST"))
        .and(path("/reseller/users/johndoe/suspend"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/reseller/users/johndoe/unsuspend"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let users = client.reseller().users();
    users.suspend("johndoe").await.expect("the call succeeds");
    users.unsuspend("johndoe").await.expect("the call succeeds");
}

#[tokio::test]
async fn reassigning_a_package_answers_with_the_updated_user() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/reseller/users/johndoe/package"))
        .and(body_json(json!({ "package": "premium" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "username": "johndoe",
            "email": "john@example.com",
            "domain": "example.com",
            "package": "premium",
            "suspended": false,
            "quota": { "limit": null, "used": 512.5, "unlimited": true },
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let user = client
        .reseller()
        .users()
        .set_package("johndoe", "premium")
        .await
        .expect("the call succeeds");
    assert_eq!(user.package, "premium");
    assert!(user.quota.unlimited);
    assert_eq!(user.quota.limit, None);
}

#[tokio::test]
async fn reassigning_to_a_package_that_does_not_exist_is_a_validation_error() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/reseller/users/johndoe/package"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(common::err("VALIDATION_ERROR", "Package does not exist")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client
        .reseller()
        .users()
        .set_package("johndoe", "absent")
        .await
        .expect_err("no such package");
    assert!(err.is_validation());
}

#[tokio::test]
async fn reading_a_user_decodes_their_quota() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/reseller/users/johndoe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(user_json("johndoe"))))
        .expect(1)
        .mount(&server)
        .await;

    let user = client
        .reseller()
        .users()
        .get("johndoe")
        .await
        .expect("the call succeeds");
    assert_eq!(user.quota.limit, Some(10240));
    assert_eq!(user.quota.used, 512.5);
}

#[tokio::test]
async fn creating_a_package_sends_flat_strings() {
    let (server, client) = mock().await;
    // Nested and typed on the way out, flat and stringly on the way in.
    Mock::given(method("POST"))
        .and(path("/reseller/packages"))
        .and(body_json(json!({
            "name": "basic",
            "quota": "10",
            "domains": "5",
            "email_accounts": "unlimited",
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let spec = PackageSpec::new()
        .quota(PackageQuota::Gigabytes(10.0))
        .domains(PackageLimit::Value(5))
        .email_accounts(PackageLimit::Unlimited);
    client
        .reseller()
        .packages()
        .create("basic", &spec)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn reading_a_package_reconciles_its_nested_settings() {
    let (server, client) = mock().await;
    Mock::given(method("GET"))
        .and(path("/reseller/packages/basic"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok(json!({
            "name": "basic",
            "settings": {
                "quota_gb": 10.0,
                "quota_unlimited": false,
                "domains": 5,
                "email_accounts": null,
                "email_forwarders": 100,
                "domain_pointers": 10,
            },
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let package = client
        .reseller()
        .packages()
        .get("basic")
        .await
        .expect("the call succeeds");
    assert_eq!(package.settings.quota(), PackageQuota::Gigabytes(10.0));
    assert_eq!(package.settings.domain_limit(), PackageLimit::Value(5));
    // A null count is unlimited, not zero.
    assert_eq!(
        package.settings.email_account_limit(),
        PackageLimit::Unlimited
    );
}

#[tokio::test]
async fn updating_a_package_omits_the_name_that_addresses_it() {
    let (server, client) = mock().await;
    Mock::given(method("PATCH"))
        .and(path("/reseller/packages/basic"))
        .and(body_json(json!({ "domain_pointers": "20" })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let spec = PackageSpec::new().domain_pointers(PackageLimit::Value(20));
    client
        .reseller()
        .packages()
        .update("basic", &spec)
        .await
        .expect("the call succeeds");
}

#[tokio::test]
async fn an_update_that_sets_nothing_is_refused_before_a_request() {
    let (_server, client) = mock().await;
    let err = client
        .reseller()
        .packages()
        .update("basic", &PackageSpec::new())
        .await
        .expect_err("an empty body is not sent");
    assert!(err.is_validation());
    assert_eq!(client.requests_made(), 0);
}

#[tokio::test]
async fn a_package_still_in_use_cannot_be_deleted() {
    let (server, client) = mock().await;
    Mock::given(method("DELETE"))
        .and(path("/reseller/packages/basic"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(common::err("VALIDATION_ERROR", "Package is in use")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client
        .reseller()
        .packages()
        .delete("basic")
        .await
        .expect_err("users are still on it");
    assert!(err.is_validation());
}
