//! Account-level information: `/verification-key`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::api::dns::DnsRecord;
use crate::client::Client;
use crate::error::Result;

/// The account's domain verification key and the record that proves ownership.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct VerificationKey {
    /// The key itself, `_da-verify-` followed by 32 hex characters.
    pub key: String,
    /// The TXT record to publish, with `name` to prepend to the domain being added.
    pub record: DnsRecord,
    /// The API's own description of what to do with it.
    #[serde(default)]
    pub description: String,
}

/// Account endpoints.
#[derive(Debug, Clone, Copy)]
pub struct AccountApi<'a> {
    client: &'a Client,
}

impl<'a> AccountApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /verification-key` — the key proving a domain may be added to this account.
    ///
    /// Adding a domain is not a single API call. Fetch the key, publish
    /// `{record.name}.{domain}` as a TXT record with `record.value`, wait for DNS to
    /// propagate — the API documents 5 to 15 minutes — and only then call
    /// [`create`](crate::api::DomainsApi::create). There is no endpoint that reports
    /// whether propagation has finished; the creation call failing is the signal.
    ///
    /// An account with no key answers `500` with
    /// [`ErrorCode::VerificationKeyNotFound`](crate::ErrorCode::VerificationKeyNotFound),
    /// which is not a transient failure and will not come good on a retry.
    pub async fn verification_key(&self) -> Result<VerificationKey> {
        let url = self.client.url(&["verification-key"]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_verification_key_decodes_with_its_record() {
        let key: VerificationKey = serde_json::from_str(
            r#"{"key": "_da-verify-a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
                "record": {"type": "TXT",
                           "name": "_da-verify-a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
                           "value": "domain-verified"},
                "description": "Add this TXT record before adding the domain"}"#,
        )
        .expect("decodes");
        assert!(key.key.starts_with("_da-verify-"));
        assert_eq!(key.record.record_type, "TXT");
        assert_eq!(key.record.value, "domain-verified");
        // The record's own description is absent here; only the outer one is set.
        assert_eq!(key.record.description, None);
    }
}
