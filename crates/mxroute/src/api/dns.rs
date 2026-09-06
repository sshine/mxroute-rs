//! The DNS records a domain needs at its registrar: `/domains/{domain}/dns`.
//!
//! Read-only. MXroute does not host the zone; this reports what to publish wherever the
//! zone actually lives.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{Result, check_path_segment};

/// A record to publish at the registrar.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct DnsRecord {
    /// The record type, such as `TXT`.
    #[serde(rename = "type")]
    pub record_type: String,
    /// The owner name, where `@` is the apex.
    pub name: String,
    /// The record's value.
    pub value: String,
    /// What the record is for, when the API explains it.
    #[serde(default)]
    pub description: Option<String>,
}

/// One of the mail exchangers to publish.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct MxRecord {
    /// Preference value; lower is tried first.
    pub priority: u16,
    /// The exchanger's hostname.
    pub hostname: String,
    /// What the record is for.
    #[serde(default)]
    pub description: Option<String>,
}

/// Everything a domain needs published for mail to work.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct DnsInfo {
    /// The mail exchangers.
    #[serde(default)]
    pub mx_records: Vec<MxRecord>,
    /// The sender policy record.
    pub spf: DnsRecord,
    /// The signing key record, absent until DKIM is set up for the domain.
    ///
    /// The API exposes no way to create or rotate this; it appears here once the domain
    /// has one.
    #[serde(default)]
    pub dkim: Option<DnsRecord>,
    /// The ownership record, absent once the domain is verified.
    #[serde(default)]
    pub verification: Option<DnsRecord>,
}

/// DNS information endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct DnsApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> DnsApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    /// `GET /domains/{domain}/dns` — the records to publish for this domain.
    pub async fn get(&self) -> Result<DnsInfo> {
        check_path_segment("domain", self.domain)?;
        let url = self.client.url(&["domains", self.domain, "dns"]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_domain_without_dkim_or_verification_decodes_with_both_absent() {
        let info: DnsInfo = serde_json::from_str(
            r#"{"mx_records": [{"priority": 10, "hostname": "eagle.mxlogin.com"}],
                "spf": {"type": "TXT", "name": "@", "value": "v=spf1 include:mxroute.com -all"},
                "dkim": null, "verification": null}"#,
        )
        .expect("both are nullable");
        assert_eq!(info.dkim, None);
        assert_eq!(info.verification, None);
        assert_eq!(info.mx_records[0].priority, 10);
        assert_eq!(info.spf.record_type, "TXT");
        // The description is optional on an MX record even when the field is absent.
        assert_eq!(info.mx_records[0].description, None);
    }
}
