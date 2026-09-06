//! Domain pointers: `/domains/{domain}/pointers`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Result, check_path_segment};

/// What a pointer does with the mail and traffic it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PointerKind {
    /// The pointer is treated as another name for the target.
    Alias,
    /// The pointer redirects to the target.
    Redirect,
}

impl PointerKind {
    /// The creation body's `alias` flag, which is how the API spells this on the way in.
    ///
    /// Reads report a `type` of `alias` or `redirect`; writes take a boolean. The two
    /// spellings are the same choice, so the enum is what callers see in both directions.
    fn is_alias(self) -> bool {
        matches!(self, Self::Alias)
    }
}

/// A name pointed at a domain.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct DomainPointer {
    /// The pointing name.
    pub pointer: String,
    /// Whether it aliases or redirects.
    #[serde(rename = "type")]
    pub kind: PointerKind,
    /// The domain it points at.
    pub target: String,
}

/// The body of a pointer creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NewPointer<'a> {
    pointer: &'a str,
    alias: bool,
}

/// Domain pointer endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct PointersApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> PointersApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    fn url(&self, tail: &[&str]) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        let mut segments = vec!["domains", self.domain, "pointers"];
        segments.extend_from_slice(tail);
        Ok(self.client.url(&segments))
    }

    /// `GET /domains/{domain}/pointers` — every pointer at this domain.
    pub async fn list(&self) -> Result<Vec<DomainPointer>> {
        let req = self.client.request(Method::GET, self.url(&[])?);
        self.client.send_json(req).await
    }

    /// `POST /domains/{domain}/pointers` — points a name at this domain.
    ///
    /// The API documents a `201` with no body.
    pub async fn create(&self, pointer: &str, kind: PointerKind) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[])?)
            .json(&NewPointer {
                pointer,
                alias: kind.is_alias(),
            })?;
        self.client.send_empty(req).await
    }

    /// `DELETE /domains/{domain}/pointers/{pointer}` — removes a pointer.
    pub async fn delete(&self, pointer: &str) -> Result<()> {
        check_path_segment("pointer", pointer)?;
        let req = self.client.request(Method::DELETE, self.url(&[pointer])?);
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_pointer_reads_as_a_kind_and_writes_as_a_flag() {
        let stored: DomainPointer = serde_json::from_str(
            r#"{"pointer": "alias.com", "type": "redirect", "target": "example.com"}"#,
        )
        .expect("decodes");
        assert_eq!(stored.kind, PointerKind::Redirect);

        let body = serde_json::to_string(&NewPointer {
            pointer: "alias.com",
            alias: PointerKind::Redirect.is_alias(),
        })
        .expect("serializes");
        assert_eq!(body, r#"{"pointer":"alias.com","alias":false}"#);
    }

    #[test]
    fn an_alias_writes_the_flag_the_api_defaults_to() {
        let body = serde_json::to_string(&NewPointer {
            pointer: "alias.com",
            alias: PointerKind::Alias.is_alias(),
        })
        .expect("serializes");
        assert_eq!(body, r#"{"pointer":"alias.com","alias":true}"#);
    }
}
