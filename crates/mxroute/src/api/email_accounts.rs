//! Mailboxes: `/domains/{domain}/email-accounts`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::{Client, Secret};
use crate::error::{Result, check_path_segment};
use crate::types::{MailboxQuota, SendLimit};

/// A mailbox.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct EmailAccount {
    /// The local part, before the `@`.
    pub username: String,
    /// The full address.
    pub email: String,
    /// The disk allowance.
    pub quota: MailboxQuota,
    /// Disk used, in megabytes. Fractional, unlike the quota.
    pub usage: f64,
    /// Messages the mailbox may send in a day.
    pub limit: SendLimit,
    /// Messages sent today, against `limit`.
    pub sent: u32,
    /// Whether the mailbox is suspended.
    pub suspended: bool,
}

/// The body of a mailbox creation request.
///
/// The password must be at least eight characters and mix upper case, lower case and a
/// digit; the API rejects it otherwise. That is not checked here, because a rule this
/// crate cannot see change is one it should not enforce on the caller's behalf.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewEmailAccount {
    username: String,
    password: Secret,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota: Option<MailboxQuota>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<SendLimit>,
}

impl NewEmailAccount {
    /// A mailbox with the API's default quota and send limit.
    pub fn new(username: impl Into<String>, password: impl Into<Secret>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            quota: None,
            limit: None,
        }
    }

    /// Sets the disk allowance. The API defaults to 1024 MB.
    pub fn quota(mut self, quota: MailboxQuota) -> Self {
        self.quota = Some(quota);
        self
    }

    /// Sets the daily send limit. The API defaults to the maximum.
    pub fn send_limit(mut self, limit: SendLimit) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Changes to an existing mailbox.
///
/// Every field is optional and omitted fields are left alone, so an update that only
/// resets a password does not have to restate the quota.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EmailAccountPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota: Option<MailboxQuota>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<SendLimit>,
}

impl EmailAccountPatch {
    /// An update that changes nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a new password.
    pub fn password(mut self, password: impl Into<Secret>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Sets a new disk allowance.
    pub fn quota(mut self, quota: MailboxQuota) -> Self {
        self.quota = Some(quota);
        self
    }

    /// Sets a new daily send limit.
    pub fn send_limit(mut self, limit: SendLimit) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Whether this update would change anything.
    ///
    /// An empty body is a request spent to no effect, and the API's own reaction to one is
    /// not documented.
    pub fn is_empty(&self) -> bool {
        self.password.is_none() && self.quota.is_none() && self.limit.is_none()
    }
}

/// Mailbox endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct EmailAccountsApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> EmailAccountsApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    fn url(&self, tail: &[&str]) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        let mut segments = vec!["domains", self.domain, "email-accounts"];
        segments.extend_from_slice(tail);
        Ok(self.client.url(&segments))
    }

    /// `GET /domains/{domain}/email-accounts` — every mailbox in the domain.
    pub async fn list(&self) -> Result<Vec<EmailAccount>> {
        let req = self.client.request(Method::GET, self.url(&[])?);
        self.client.send_json(req).await
    }

    /// `GET /domains/{domain}/email-accounts/{user}` — one mailbox.
    pub async fn get(&self, username: &str) -> Result<EmailAccount> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::GET, self.url(&[username])?);
        self.client.send_json(req).await
    }

    /// As [`get`](Self::get), with `404` mapped onto `None`.
    pub async fn try_get(&self, username: &str) -> Result<Option<EmailAccount>> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::GET, self.url(&[username])?);
        self.client.send_json_opt(req).await
    }

    /// `POST /domains/{domain}/email-accounts` — creates a mailbox.
    ///
    /// The API documents a `201` with no body, so the new mailbox is not returned; read it
    /// back with [`get`](Self::get) if its stored form matters.
    pub async fn create(&self, account: &NewEmailAccount) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[])?)
            .json(account)?;
        self.client.send_empty(req).await
    }

    /// `PATCH /domains/{domain}/email-accounts/{user}` — changes a mailbox.
    ///
    /// A patch that would change nothing is refused locally rather than sent.
    pub async fn update(&self, username: &str, patch: &EmailAccountPatch) -> Result<()> {
        check_path_segment("username", username)?;
        if patch.is_empty() {
            return Err(
                crate::error::InvalidValue::new("patch", "would change nothing", "{}").into(),
            );
        }
        let req = self
            .client
            .request(Method::PATCH, self.url(&[username])?)
            .json(patch)?;
        self.client.send_empty(req).await
    }

    /// `DELETE /domains/{domain}/email-accounts/{user}` — removes a mailbox and its mail.
    pub async fn delete(&self, username: &str) -> Result<()> {
        check_path_segment("username", username)?;
        let req = self.client.request(Method::DELETE, self.url(&[username])?);
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_plain_creation_sends_the_two_required_fields_and_nothing_else() {
        let body = serde_json::to_string(&NewEmailAccount::new("sales", "Hunter2Hunter2"))
            .expect("serializes");
        assert_eq!(body, r#"{"username":"sales","password":"Hunter2Hunter2"}"#);
    }

    #[test]
    fn an_unlimited_quota_is_sent_as_zero_not_omitted() {
        // Omitting it would take the API's 1024 MB default, which is the opposite of what
        // the caller asked for.
        let account =
            NewEmailAccount::new("sales", "Hunter2Hunter2").quota(MailboxQuota::Unlimited);
        let body = serde_json::to_string(&account).expect("serializes");
        assert!(body.contains(r#""quota":0"#), "{body}");
    }

    #[test]
    fn an_empty_patch_serializes_to_an_empty_object_and_knows_it() {
        let patch = EmailAccountPatch::new();
        assert!(patch.is_empty());
        assert_eq!(serde_json::to_string(&patch).expect("serializes"), "{}");
    }

    #[test]
    fn a_patch_carries_only_what_was_set() {
        let patch = EmailAccountPatch::new().quota(MailboxQuota::Megabytes(2048));
        assert!(!patch.is_empty());
        let body = serde_json::to_string(&patch).expect("serializes");
        assert_eq!(body, r#"{"quota":2048}"#);
    }

    #[test]
    fn a_patch_does_not_render_the_password_it_carries() {
        let patch = EmailAccountPatch::new().password("Hunter2Hunter2");
        assert!(!format!("{patch:?}").contains("Hunter2"));
        // It still has to serialize, or the password could never be changed.
        let body = serde_json::to_string(&patch).expect("serializes");
        assert!(body.contains("Hunter2Hunter2"), "{body}");
    }

    #[test]
    fn a_mailbox_decodes_its_zero_quota_as_unlimited() {
        let account: EmailAccount = serde_json::from_str(
            r#"{"username": "sales", "email": "sales@example.com", "quota": 0,
                "usage": 256.5, "limit": 9600, "sent": 42, "suspended": false}"#,
        )
        .expect("decodes");
        assert_eq!(account.quota, MailboxQuota::Unlimited);
        assert_eq!(account.usage, 256.5);
        assert_eq!(account.limit, SendLimit::max());
    }
}
