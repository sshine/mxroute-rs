//! Email forwarders: `/domains/{domain}/forwarders`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{InvalidValue, Result, check_path_segment};
use crate::types::Destination;

/// A forwarding rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Forwarder {
    /// The local part, before the `@`.
    pub alias: String,
    /// The full address mail arrives at.
    pub email: String,
    /// Where the mail goes.
    pub destinations: Vec<Destination>,
}

/// The body of a forwarder creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewForwarder {
    alias: String,
    destinations: Vec<Destination>,
}

impl NewForwarder {
    /// A forwarder from `alias` to one or more destinations.
    ///
    /// Fails when `destinations` is empty: the API requires the field, and a forwarder
    /// with nowhere to forward has no meaning. Use [`Destination::Blackhole`] to discard
    /// mail deliberately.
    pub fn new<D>(
        alias: impl Into<String>,
        destinations: impl IntoIterator<Item = D>,
    ) -> Result<Self>
    where
        D: Into<Destination>,
    {
        let destinations: Vec<Destination> = destinations.into_iter().map(Into::into).collect();
        if destinations.is_empty() {
            return Err(InvalidValue::new(
                "destinations",
                "must name at least one destination",
                "[]",
            )
            .into());
        }
        Ok(Self {
            alias: alias.into(),
            destinations,
        })
    }
}

/// Forwarder endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct ForwardersApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> ForwardersApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    fn url(&self, tail: &[&str]) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        let mut segments = vec!["domains", self.domain, "forwarders"];
        segments.extend_from_slice(tail);
        Ok(self.client.url(&segments))
    }

    /// `GET /domains/{domain}/forwarders` — every forwarder in the domain.
    pub async fn list(&self) -> Result<Vec<Forwarder>> {
        let req = self.client.request(Method::GET, self.url(&[])?);
        self.client.send_json(req).await
    }

    /// `POST /domains/{domain}/forwarders` — creates a forwarder.
    ///
    /// Forwarding to Gmail, Yahoo, AOL or a similar provider turns on Expert Spam
    /// Filtering for the whole domain, not just this forwarder. That is a side effect on
    /// every other address in the domain, and the API applies it without saying so in the
    /// response.
    ///
    /// The API documents a `201` with no body, so the stored forwarder is not returned.
    pub async fn create(&self, forwarder: &NewForwarder) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[])?)
            .json(forwarder)?;
        self.client.send_empty(req).await
    }

    /// `DELETE /domains/{domain}/forwarders/{alias}` — removes a forwarder.
    pub async fn delete(&self, alias: &str) -> Result<()> {
        check_path_segment("alias", alias)?;
        let req = self.client.request(Method::DELETE, self.url(&[alias])?);
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_forwarder_serializes_its_destinations_as_bare_strings() {
        let forwarder =
            NewForwarder::new("sales", ["someone@example.com"]).expect("one destination");
        let body = serde_json::to_string(&forwarder).expect("serializes");
        assert_eq!(
            body,
            r#"{"alias":"sales","destinations":["someone@example.com"]}"#
        );
    }

    #[test]
    fn the_magic_destinations_survive_the_round_trip() {
        let forwarder =
            NewForwarder::new("noreply", [Destination::Blackhole]).expect("one destination");
        let body = serde_json::to_string(&forwarder).expect("serializes");
        assert!(body.contains(r#"":blackhole:""#), "{body}");
    }

    #[test]
    fn a_forwarder_with_nowhere_to_forward_is_refused() {
        let empty: [&str; 0] = [];
        assert!(NewForwarder::new("sales", empty).is_err());
    }

    #[test]
    fn a_stored_forwarder_decodes_its_magic_destinations() {
        let forwarder: Forwarder = serde_json::from_str(
            r#"{"alias": "noreply", "email": "noreply@example.com",
                "destinations": [":fail:", "someone@example.com"]}"#,
        )
        .expect("decodes");
        assert_eq!(
            forwarder.destinations,
            vec![
                Destination::Fail,
                Destination::address("someone@example.com")
            ]
        );
    }
}
