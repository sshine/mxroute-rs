//! Catch-all handling: `/domains/{domain}/catch-all`.

use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::client::Client;
use crate::error::{Result, check_path_segment};

/// What happens to mail for an address that does not exist.
///
/// The wire form is a `type` string beside a nullable `address`, where the address is
/// required for exactly one of the three types and meaningless for the other two. Carrying
/// the address inside the variant that needs it is what stops `Address` without one, and
/// `Fail` with one, from being expressible.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatchAll {
    /// Reject the message at delivery.
    Fail,
    /// Accept the message and discard it.
    Blackhole,
    /// Deliver it to this address.
    Address(String),
}

/// The wire form of a catch-all, in both directions.
#[derive(Deserialize, Serialize)]
struct Wire {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    address: Option<String>,
}

impl Serialize for CatchAll {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Fail => Wire {
                kind: "fail".to_owned(),
                address: None,
            },
            Self::Blackhole => Wire {
                kind: "blackhole".to_owned(),
                address: None,
            },
            Self::Address(address) => Wire {
                kind: "address".to_owned(),
                address: Some(address.clone()),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CatchAll {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        let wire = Wire::deserialize(deserializer)?;
        match wire.kind.as_str() {
            "fail" => Ok(Self::Fail),
            "blackhole" => Ok(Self::Blackhole),
            "address" => wire.address.map(Self::Address).ok_or_else(|| {
                D::Error::custom("catch-all type \"address\" without an address to deliver to")
            }),
            other => Err(D::Error::custom(format!(
                "unknown catch-all type {other:?}"
            ))),
        }
    }
}

/// A domain's catch-all setting, as the API reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CatchAllSetting {
    /// What happens to the mail.
    pub catch_all: CatchAll,
    /// The API's own description of the setting, meant for showing to a user.
    pub description: String,
}

impl<'de> Deserialize<'de> for CatchAllSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            address: Option<String>,
            #[serde(default)]
            description: String,
        }

        let raw = Raw::deserialize(deserializer)?;
        let catch_all = CatchAll::deserialize(serde_json::json!({
            "type": raw.kind,
            "address": raw.address,
        }))
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            catch_all,
            description: raw.description,
        })
    }
}

/// Catch-all endpoints, scoped to one domain.
#[derive(Debug, Clone, Copy)]
pub struct CatchAllApi<'a> {
    client: &'a Client,
    domain: &'a str,
}

impl<'a> CatchAllApi<'a> {
    pub(crate) fn new(client: &'a Client, domain: &'a str) -> Self {
        Self { client, domain }
    }

    fn url(&self) -> Result<url::Url> {
        check_path_segment("domain", self.domain)?;
        Ok(self.client.url(&["domains", self.domain, "catch-all"]))
    }

    /// `GET /domains/{domain}/catch-all` — the current setting.
    pub async fn get(&self) -> Result<CatchAllSetting> {
        let req = self.client.request(Method::GET, self.url()?);
        self.client.send_json(req).await
    }

    /// `PATCH /domains/{domain}/catch-all` — changes the setting.
    ///
    /// The API documents a `200` with no body.
    pub async fn set(&self, catch_all: &CatchAll) -> Result<()> {
        let req = self
            .client
            .request(Method::PATCH, self.url()?)
            .json(catch_all)?;
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn the_two_addressless_types_send_no_address_field() {
        assert_eq!(
            serde_json::to_string(&CatchAll::Fail).expect("serializes"),
            r#"{"type":"fail"}"#
        );
        assert_eq!(
            serde_json::to_string(&CatchAll::Blackhole).expect("serializes"),
            r#"{"type":"blackhole"}"#
        );
    }

    #[test]
    fn an_address_type_carries_the_address_it_needs() {
        let body = serde_json::to_string(&CatchAll::Address("me@example.com".to_owned()))
            .expect("serializes");
        assert_eq!(body, r#"{"type":"address","address":"me@example.com"}"#);
    }

    #[test]
    fn a_catch_all_round_trips() {
        for catch_all in [
            CatchAll::Fail,
            CatchAll::Blackhole,
            CatchAll::Address("me@example.com".to_owned()),
        ] {
            let json = serde_json::to_string(&catch_all).expect("serializes");
            let back: CatchAll = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, catch_all);
        }
    }

    #[test]
    fn a_null_address_beside_a_fail_type_decodes() {
        let catch_all: CatchAll =
            serde_json::from_str(r#"{"type": "fail", "address": null}"#).expect("decodes");
        assert_eq!(catch_all, CatchAll::Fail);
    }

    #[test]
    fn an_address_type_without_an_address_is_refused_rather_than_defaulted() {
        // Defaulting to an empty address would send mail nowhere and report success.
        let err = serde_json::from_str::<CatchAll>(r#"{"type": "address", "address": null}"#)
            .expect_err("the combination is not representable");
        assert!(err.to_string().contains("without an address"), "{err}");
    }

    #[test]
    fn an_unknown_type_is_refused() {
        assert!(serde_json::from_str::<CatchAll>(r#"{"type": "bounce"}"#).is_err());
    }

    #[test]
    fn a_setting_decodes_alongside_its_description() {
        let setting: CatchAllSetting = serde_json::from_str(
            r#"{"type": "address", "address": "me@example.com",
                "description": "Delivered to me@example.com"}"#,
        )
        .expect("decodes");
        assert_eq!(
            setting.catch_all,
            CatchAll::Address("me@example.com".to_owned())
        );
        assert_eq!(setting.description, "Delivered to me@example.com");
    }
}
