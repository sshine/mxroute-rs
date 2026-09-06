//! Spam filtering: `/domains/{domain}/spam`.
//!
//! Two things about this group are not like the rest of the API, and both can lose data.
//!
//! **The sender lists are account-wide.** They are reached through a domain, and the API
//! requires that the domain be one of yours, but whitelisting `*@trusted.com` under
//! `a.example` also whitelists it for `b.example` and every other domain on the account.
//! Removing an entry under one domain removes it everywhere.
//!
//! **Writes here are not safely repeatable.** Updates are serialized against the mxpanel,
//! so a write can answer `503` ([`Error::is_busy`](crate::Error::is_busy)) when the panel
//! holds the lock, and a `500` means the result could not be confirmed rather than that
//! nothing happened. The client never replays these on its own. After a failed write, read
//! the settings back and decide from what is actually stored.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Result, check_path_segment};
use crate::types::{SpamEntry, SpamScore};

/// A domain's spam filter settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct SpamSettings {
    /// The score at or above which a message is treated as spam and deleted.
    pub high_score: SpamScore,
}

/// The body of a settings update.
///
/// The API refuses unknown members outright, so this carries the one field and no more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SpamSettingsPatch {
    high_score: SpamScore,
}

/// The body of a sender-list addition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NewEntry<'a> {
    entry: &'a SpamEntry,
}

/// Which of the two sender lists a handle addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SenderList {
    Whitelist,
    Blacklist,
}

impl SenderList {
    fn as_segment(self) -> &'static str {
        match self {
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
        }
    }
}

/// Spam endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct SpamApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> SpamApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    fn settings_url(&self) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        Ok(self
            .client
            .url(&["domains", self.domain, "spam", "settings"]))
    }

    /// `GET /domains/{domain}/spam/settings` — the current threshold.
    pub async fn settings(&self) -> Result<SpamSettings> {
        let req = self.client.request(Method::GET, self.settings_url()?);
        self.client.send_json(req).await
    }

    /// `PATCH /domains/{domain}/spam/settings` — changes the threshold.
    ///
    /// Not retried by the client, and not safe for a caller to retry blindly: see the
    /// module documentation. On any failure, call [`settings`](Self::settings) and decide
    /// from what is stored.
    pub async fn set_high_score(&self, high_score: SpamScore) -> Result<()> {
        let req = self
            .client
            .request(Method::PATCH, self.settings_url()?)
            .json(&SpamSettingsPatch { high_score })?;
        self.client.send_empty(req).await
    }

    /// Senders whose mail always passes the filter.
    ///
    /// Shared with every other domain on the account.
    pub fn whitelist(&self) -> SenderListApi<'a> {
        SenderListApi {
            client: self.client,
            domain: self.domain,
            list: SenderList::Whitelist,
        }
    }

    /// Senders whose mail is always treated as spam.
    ///
    /// Shared with every other domain on the account.
    pub fn blacklist(&self) -> SenderListApi<'a> {
        SenderListApi {
            client: self.client,
            domain: self.domain,
            list: SenderList::Blacklist,
        }
    }
}

/// One of the two sender lists.
///
/// Reached through a domain but stored per account: an entry added here applies to every
/// domain on the account, and removing one removes it everywhere.
#[derive(Debug, Clone, Copy)]
pub struct SenderListApi<'a> {
    client: &'a Client,
    domain: &'a str,
    list: SenderList,
}

impl SenderListApi<'_> {
    fn url(&self, tail: &[&str]) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        let mut segments = vec!["domains", self.domain, "spam", self.list.as_segment()];
        segments.extend_from_slice(tail);
        Ok(self.client.url(&segments))
    }

    /// `GET /domains/{domain}/spam/{list}` — every entry on the list.
    pub async fn list(&self) -> Result<Vec<SpamEntry>> {
        let req = self.client.request(Method::GET, self.url(&[])?);
        self.client.send_json(req).await
    }

    /// `POST /domains/{domain}/spam/{list}` — adds an entry.
    ///
    /// Fails with [`is_conflict`](crate::Error::is_conflict) when the entry is already
    /// there, compared without regard to case.
    pub async fn add(&self, entry: &SpamEntry) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[])?)
            .json(&NewEntry { entry })?;
        self.client.send_empty(req).await
    }

    /// `DELETE /domains/{domain}/spam/{list}/{entry}` — removes an entry.
    ///
    /// The entry travels in the path. [`SpamEntry`] has already rejected anything that
    /// would not survive as one segment.
    pub async fn remove(&self, entry: &SpamEntry) -> Result<()> {
        let req = self
            .client
            .request(Method::DELETE, self.url(&[entry.as_str()])?);
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{Credentials, client::Client};

    fn client() -> Client {
        Client::builder()
            .credentials(Credentials::new("eagle.mxlogin.com", "johndoe", "key"))
            .build()
            .expect("valid configuration")
    }

    #[test]
    fn a_settings_patch_carries_only_the_one_field_the_api_permits() {
        let body = serde_json::to_string(&SpamSettingsPatch {
            high_score: SpamScore::new(5).expect("in range"),
        })
        .expect("serializes");
        assert_eq!(body, r#"{"high_score":5}"#);
    }

    #[test]
    fn an_addition_carries_the_entry_as_a_bare_string() {
        let entry = SpamEntry::new("*@trusted.com").expect("valid entry");
        let body = serde_json::to_string(&NewEntry { entry: &entry }).expect("serializes");
        assert_eq!(body, r#"{"entry":"*@trusted.com"}"#);
    }

    #[test]
    fn the_two_lists_address_different_paths() {
        let client = client();
        let spam = SpamApi::new(&client, "example.com");
        assert!(
            spam.whitelist()
                .url(&[])
                .expect("valid domain")
                .path()
                .ends_with("/spam/whitelist")
        );
        assert!(
            spam.blacklist()
                .url(&[])
                .expect("valid domain")
                .path()
                .ends_with("/spam/blacklist")
        );
    }

    #[test]
    fn a_wildcard_entry_survives_the_deletion_path_as_one_segment() {
        let client = client();
        let entry = SpamEntry::new("*@trusted.com").expect("valid entry");
        let url = SpamApi::new(&client, "example.com")
            .whitelist()
            .url(&[entry.as_str()])
            .expect("valid domain");
        assert_eq!(
            url.path_segments().map(Iterator::collect::<Vec<_>>),
            Some(vec![
                "domains",
                "example.com",
                "spam",
                "whitelist",
                "*@trusted.com"
            ])
        );
    }

    #[test]
    fn settings_decode_their_score() {
        let settings: SpamSettings = serde_json::from_str(r#"{"high_score": 7}"#).expect("decodes");
        assert_eq!(settings.high_score.get(), 7);
    }
}
