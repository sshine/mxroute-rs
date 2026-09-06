//! Domain management: `/domains`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Result, check_path_segment};

/// A domain on the account.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Domain {
    /// The domain name.
    pub domain: String,
    /// Whether this domain's mail is hosted here.
    pub mail_hosting: bool,
    /// Whether a certificate is installed for it.
    pub ssl_enabled: bool,
    /// Names that resolve to this domain, as bare strings.
    #[serde(default)]
    pub pointers: Vec<String>,
}

/// What `POST /domains` answers with.
///
/// Narrower than [`Domain`]: creation reports only the name and whether a certificate
/// came with it. Read the domain back for the rest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct CreatedDomain {
    /// The domain name, as the API recorded it.
    pub domain: String,
    /// Whether a certificate is installed for it.
    pub ssl_enabled: bool,
}

/// The body of a domain creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NewDomain<'a> {
    domain: &'a str,
}

/// The body of a mail-hosting update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MailStatus {
    enabled: bool,
}

/// Domain endpoints.
#[derive(Debug, Clone, Copy)]
pub struct DomainsApi<'a> {
    client: &'a Client,
}

impl<'a> DomainsApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /domains` — the names of every domain on the account.
    ///
    /// Names only: the API answers this one with an array of strings rather than of
    /// objects. Use [`get`](Self::get) for a domain's settings.
    pub async fn list(&self) -> Result<Vec<String>> {
        let url = self.client.url(&["domains"]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json(req).await
    }

    /// `POST /domains` — adds a domain to the account.
    ///
    /// The domain must already carry the account's verification TXT record, published far
    /// enough ahead for DNS to have caught up; the API rejects the request otherwise.
    /// Fails with [`is_conflict`](crate::Error::is_conflict) when the domain is already on
    /// the account.
    pub async fn create(&self, domain: &str) -> Result<CreatedDomain> {
        let url = self.client.url(&["domains"]);
        let req = self
            .client
            .request(Method::POST, url)
            .json(&NewDomain { domain })?;
        self.client.send_json(req).await
    }

    /// `GET /domains/{domain}` — one domain's settings.
    ///
    /// Answers `404` both for a domain that does not exist and for one held by another
    /// account, so a `404` is not evidence that the name is free.
    pub async fn get(&self, domain: &str) -> Result<Domain> {
        check_path_segment("domain", domain)?;
        let url = self.client.url(&["domains", domain]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json(req).await
    }

    /// As [`get`](Self::get), with `404` mapped onto `None`.
    pub async fn try_get(&self, domain: &str) -> Result<Option<Domain>> {
        check_path_segment("domain", domain)?;
        let url = self.client.url(&["domains", domain]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json_opt(req).await
    }

    /// `DELETE /domains/{domain}` — removes a domain and the mail in it.
    pub async fn delete(&self, domain: &str) -> Result<()> {
        check_path_segment("domain", domain)?;
        let url = self.client.url(&["domains", domain]);
        let req = self.client.request(Method::DELETE, url);
        self.client.send_empty(req).await
    }

    /// `PATCH /domains/{domain}/mail-status` — turns mail hosting on or off.
    ///
    /// Disabling stops delivery for the domain; it does not remove the mailboxes.
    pub async fn set_mail_hosting(&self, domain: &str, enabled: bool) -> Result<()> {
        check_path_segment("domain", domain)?;
        let url = self.client.url(&["domains", domain, "mail-status"]);
        let req = self
            .client
            .request(Method::PATCH, url)
            .json(&MailStatus { enabled })?;
        // The API documents a `200` with no body for this one.
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_creation_body_carries_only_the_name() {
        let body = serde_json::to_string(&NewDomain {
            domain: "example.com",
        })
        .expect("the body serializes");
        assert_eq!(body, r#"{"domain":"example.com"}"#);
    }

    #[test]
    fn a_mail_status_body_sends_the_flag_rather_than_omitting_it() {
        // Omission would read as "leave unchanged", which is not how the field is
        // specified: it is required, so disabling has to be expressible.
        let body =
            serde_json::to_string(&MailStatus { enabled: false }).expect("the body serializes");
        assert_eq!(body, r#"{"enabled":false}"#);
    }

    #[test]
    fn a_domain_without_pointers_decodes_with_an_empty_list() {
        let domain: Domain = serde_json::from_str(
            r#"{"domain": "example.com", "mail_hosting": true, "ssl_enabled": false}"#,
        )
        .expect("the field is optional");
        assert!(domain.pointers.is_empty());
    }
}
