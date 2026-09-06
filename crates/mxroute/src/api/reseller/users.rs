//! Users under a reseller: `/reseller/users`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::{Client, Secret};
use crate::error::{InvalidValue, Result, check_path_segment};

/// Longest username the API accepts.
pub const MAX_USERNAME_LEN: usize = 10;

/// A user's disk allowance, as the API reports it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct UserQuota {
    /// The allowance in megabytes, absent when there is none.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Megabytes used.
    pub used: f64,
    /// Whether the allowance is unlimited.
    pub unlimited: bool,
}

/// A user under the reseller.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ResellerUser {
    /// The DirectAdmin username.
    pub username: String,
    /// The contact address.
    pub email: String,
    /// The user's primary domain.
    pub domain: String,
    /// The package assigned to them.
    pub package: String,
    /// Whether the account is suspended.
    pub suspended: bool,
    /// Their disk allowance and usage.
    pub quota: UserQuota,
}

/// The body of a user creation request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewResellerUser {
    username: String,
    email: String,
    password: Secret,
    package: String,
}

impl NewResellerUser {
    /// A new user on `package`.
    ///
    /// The username is checked against the documented rule — one to ten characters of
    /// lowercase letters, digits and underscores — because it also becomes a path segment
    /// on every later call, and because being told by the server costs a request.
    ///
    /// The password rule (at least eight characters) is left to the server, as it is for
    /// mailboxes.
    pub fn new(
        username: impl Into<String>,
        email: impl Into<String>,
        password: impl Into<Secret>,
        package: impl Into<String>,
    ) -> Result<Self> {
        let username = username.into();
        if username.is_empty() || username.len() > MAX_USERNAME_LEN {
            return Err(
                InvalidValue::new("username", "must be 1 to 10 characters", username).into(),
            );
        }
        if !username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(InvalidValue::new(
                "username",
                "may only contain lowercase letters, digits and underscores",
                username,
            )
            .into());
        }
        Ok(Self {
            username,
            email: email.into(),
            password: password.into(),
            package: package.into(),
        })
    }
}

/// Changes to an existing user.
///
/// The quota here is in **megabytes**, unlike a package's, which is in gigabytes.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResellerUserPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    quota: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<Secret>,
}

impl ResellerUserPatch {
    /// An update that changes nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the disk allowance, in megabytes.
    pub fn quota_megabytes(mut self, megabytes: u64) -> Self {
        self.quota = Some(megabytes.to_string());
        self
    }

    /// Lifts the disk allowance.
    pub fn unlimited_quota(mut self) -> Self {
        self.quota = Some("unlimited".to_owned());
        self
    }

    /// Sets a new password.
    pub fn password(mut self, password: impl Into<Secret>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Whether this update would change anything.
    pub fn is_empty(&self) -> bool {
        self.quota.is_none() && self.password.is_none()
    }
}

/// The body of a package reassignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PackageAssignment<'a> {
    package: &'a str,
}

/// Reseller user endpoints.
#[derive(Debug, Clone, Copy)]
pub struct UsersApi<'a> {
    client: &'a Client,
}

impl<'a> UsersApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    fn url(&self, tail: &[&str]) -> url::Url {
        let mut segments = vec!["reseller", "users"];
        segments.extend_from_slice(tail);
        self.client.url(&segments)
    }

    /// `GET /reseller/users` — the usernames under this reseller.
    ///
    /// Names only, the way `GET /domains` reports domains. Use [`get`](Self::get) for a
    /// user's settings.
    pub async fn list(&self) -> Result<Vec<String>> {
        let req = self.client.request(Method::GET, self.url(&[]));
        self.client.send_json(req).await
    }

    /// `GET /reseller/users/{username}` — one user.
    pub async fn get(&self, username: &str) -> Result<ResellerUser> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::GET, self.url(&[username]));
        self.client.send_json(req).await
    }

    /// As [`get`](Self::get), with `404` mapped onto `None`.
    pub async fn try_get(&self, username: &str) -> Result<Option<ResellerUser>> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::GET, self.url(&[username]));
        self.client.send_json_opt(req).await
    }

    /// `POST /reseller/users` — creates a user.
    ///
    /// The API documents a `201` with no body.
    pub async fn create(&self, user: &NewResellerUser) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[]))
            .json(user)?;
        self.client.send_empty(req).await
    }

    /// `PATCH /reseller/users/{username}` — changes a user's quota or password.
    ///
    /// A patch that would change nothing is refused locally rather than sent.
    pub async fn update(&self, username: &str, patch: &ResellerUserPatch) -> Result<()> {
        check_path_segment("username", username)?;
        if patch.is_empty() {
            return Err(InvalidValue::new("patch", "would change nothing", "{}").into());
        }
        let req = self
            .client
            .request(Method::PATCH, self.url(&[username]))
            .json(patch)?;
        self.client.send_empty(req).await
    }

    /// `DELETE /reseller/users/{username}` — removes a user and everything they hold.
    pub async fn delete(&self, username: &str) -> Result<()> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::DELETE, self.url(&[username]));
        self.client.send_empty(req).await
    }

    /// `POST /reseller/users/{username}/suspend` — suspends a user.
    ///
    /// An action of its own rather than a field on the user, which is how the API models
    /// it: there is no way to set `suspended` through [`update`](Self::update).
    pub async fn suspend(&self, username: &str) -> Result<()> {
        check_path_segment("username", username)?;
        let req = self
            .client
            .request(Method::POST, self.url(&[username, "suspend"]));
        self.client.send_empty(req).await
    }

    /// `POST /reseller/users/{username}/unsuspend` — lifts a suspension.
    pub async fn unsuspend(&self, username: &str) -> Result<()> {
        check_path_segment("username", username)?;
        let req = self
            .client
            .request(Method::POST, self.url(&[username, "unsuspend"]));
        self.client.send_empty(req).await
    }

    /// `PATCH /reseller/users/{username}/package` — moves a user to another package.
    ///
    /// The only write in this group that answers with the updated user. Fails with
    /// [`is_validation`](crate::Error::is_validation) when no such package exists.
    pub async fn set_package(&self, username: &str, package: &str) -> Result<ResellerUser> {
        check_path_segment("username", username)?;
        let req = self
            .client
            .request(Method::PATCH, self.url(&[username, "package"]))
            .json(&PackageAssignment { package })?;
        self.client.send_json(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_creation_body_carries_all_four_required_fields() {
        let user = NewResellerUser::new("johndoe", "john@example.com", "Hunter2Hunter2", "basic")
            .expect("a valid username");
        let body = serde_json::to_string(&user).expect("serializes");
        assert_eq!(
            body,
            r#"{"username":"johndoe","email":"john@example.com","password":"Hunter2Hunter2","package":"basic"}"#
        );
    }

    #[test]
    fn a_username_outside_the_documented_rule_is_refused() {
        for username in ["", "toolongusername", "JohnDoe", "john-doe", "john.doe"] {
            assert!(
                NewResellerUser::new(username, "a@b.com", "Hunter2Hunter2", "basic").is_err(),
                "{username:?} should be refused"
            );
        }
        for username in ["j", "john_doe1", "0123456789"] {
            assert!(
                NewResellerUser::new(username, "a@b.com", "Hunter2Hunter2", "basic").is_ok(),
                "{username:?} should be accepted"
            );
        }
    }

    #[test]
    fn a_quota_patch_sends_megabytes_as_a_string() {
        // The API takes this field as a string even though it holds a number, and the
        // unit is megabytes here while a package's quota is gigabytes.
        let patch = ResellerUserPatch::new().quota_megabytes(2048);
        assert_eq!(
            serde_json::to_string(&patch).expect("serializes"),
            r#"{"quota":"2048"}"#
        );
    }

    #[test]
    fn an_unlimited_quota_patch_sends_the_magic_word() {
        let patch = ResellerUserPatch::new().unlimited_quota();
        assert_eq!(
            serde_json::to_string(&patch).expect("serializes"),
            r#"{"quota":"unlimited"}"#
        );
    }

    #[test]
    fn an_empty_patch_knows_it_would_change_nothing() {
        assert!(ResellerUserPatch::new().is_empty());
        assert!(!ResellerUserPatch::new().unlimited_quota().is_empty());
    }

    #[test]
    fn a_patch_does_not_render_the_password_it_carries() {
        let patch = ResellerUserPatch::new().password("Hunter2Hunter2");
        assert!(!format!("{patch:?}").contains("Hunter2"));
    }

    #[test]
    fn a_user_decodes_with_a_null_quota_limit() {
        let user: ResellerUser = serde_json::from_str(
            r#"{"username": "johndoe", "email": "john@example.com", "domain": "example.com",
                "package": "basic", "suspended": false,
                "quota": {"limit": null, "used": 12.5, "unlimited": true}}"#,
        )
        .expect("the limit is nullable");
        assert_eq!(user.quota.limit, None);
        assert!(user.quota.unlimited);
        assert_eq!(user.quota.used, 12.5);
    }
}
