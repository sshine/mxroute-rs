//! Reseller packages: `/reseller/packages`.
//!
//! Packages read and write differently, and not just in spelling. A stored package reports
//! nested, typed settings, with `null` and a separate boolean standing for "unlimited". A
//! package being created or changed takes those same settings flat and as strings, with the
//! literal `"unlimited"`. [`PackageQuota`] and [`PackageLimit`] are what bridge the two, so
//! a caller never writes `"unlimited"` by hand or mistakes a `null` for a zero.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::{InvalidValue, Result, check_path_segment};

/// A package's disk allowance, in gigabytes.
///
/// Gigabytes, unlike the megabytes a reseller user's quota is expressed in.
///
/// This deliberately does not implement `Eq`: the API reports the allowance as a JSON
/// number and a fractional package quota is expressible, so the value is an `f64` and
/// carries that restriction with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PackageQuota {
    /// No limit. Written as `"unlimited"`.
    Unlimited,
    /// A limit in gigabytes.
    Gigabytes(f64),
}

impl PackageQuota {
    /// Reads the stored form: a nullable number beside an explicit flag.
    fn from_wire(gigabytes: Option<f64>, unlimited: bool) -> Self {
        match (unlimited, gigabytes) {
            (true, _) | (_, None) => Self::Unlimited,
            (false, Some(gb)) => Self::Gigabytes(gb),
        }
    }

    /// The written form, which is a string either way.
    fn to_wire(self) -> String {
        match self {
            Self::Unlimited => "unlimited".to_owned(),
            Self::Gigabytes(gb) => {
                // Whole numbers are the ordinary case and `1` reads better than `1.0`.
                if gb.fract() == 0.0 && gb.is_finite() {
                    format!("{gb:.0}")
                } else {
                    gb.to_string()
                }
            }
        }
    }

    /// The allowance in gigabytes, or `None` when there is no limit.
    pub fn gigabytes(self) -> Option<f64> {
        match self {
            Self::Unlimited => None,
            Self::Gigabytes(gb) => Some(gb),
        }
    }
}

/// A cap on how many of something a package allows.
///
/// The stored form is a nullable integer where `null` means unlimited; the written form is
/// a string, either a number or `"unlimited"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageLimit {
    /// No limit. Stored as `null`, written as `"unlimited"`.
    Unlimited,
    /// A limit.
    Value(u32),
}

impl PackageLimit {
    /// Reads the stored form, where `null` is unlimited.
    fn from_wire(value: Option<u32>) -> Self {
        match value {
            None => Self::Unlimited,
            Some(value) => Self::Value(value),
        }
    }

    /// The written form.
    fn to_wire(self) -> String {
        match self {
            Self::Unlimited => "unlimited".to_owned(),
            Self::Value(value) => value.to_string(),
        }
    }

    /// The limit, or `None` when there is none.
    pub fn value(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Value(value) => Some(value),
        }
    }
}

/// What a package allows, as the API stores it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct PackageSettings {
    /// Disk allowance in gigabytes, `null` when unlimited.
    #[serde(default)]
    pub quota_gb: Option<f64>,
    /// Whether the disk allowance is unlimited.
    #[serde(default)]
    pub quota_unlimited: bool,
    /// Domains allowed, `null` when unlimited.
    #[serde(default)]
    pub domains: Option<u32>,
    /// Mailboxes allowed, `null` when unlimited.
    #[serde(default)]
    pub email_accounts: Option<u32>,
    /// Forwarders allowed, `null` when unlimited.
    #[serde(default)]
    pub email_forwarders: Option<u32>,
    /// Domain pointers allowed, `null` when unlimited.
    #[serde(default)]
    pub domain_pointers: Option<u32>,
}

impl PackageSettings {
    /// The disk allowance, with the nullable number and the flag reconciled.
    pub fn quota(&self) -> PackageQuota {
        PackageQuota::from_wire(self.quota_gb, self.quota_unlimited)
    }

    /// Domains allowed.
    pub fn domain_limit(&self) -> PackageLimit {
        PackageLimit::from_wire(self.domains)
    }

    /// Mailboxes allowed.
    pub fn email_account_limit(&self) -> PackageLimit {
        PackageLimit::from_wire(self.email_accounts)
    }

    /// Forwarders allowed.
    pub fn email_forwarder_limit(&self) -> PackageLimit {
        PackageLimit::from_wire(self.email_forwarders)
    }

    /// Domain pointers allowed.
    pub fn domain_pointer_limit(&self) -> PackageLimit {
        PackageLimit::from_wire(self.domain_pointers)
    }
}

/// A package a reseller can assign to a user.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Package {
    /// The package name.
    pub name: String,
    /// What it allows.
    pub settings: PackageSettings,
}

/// The flat, stringly-typed body both package writes take.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct PackageBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_accounts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_forwarders: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain_pointers: Option<String>,
}

impl PackageBody {
    /// Whether anything but the name is set.
    fn has_settings(&self) -> bool {
        self.quota.is_some()
            || self.domains.is_some()
            || self.email_accounts.is_some()
            || self.email_forwarders.is_some()
            || self.domain_pointers.is_some()
    }
}

/// The settings of a package being created or changed.
///
/// Omitted settings take the API's defaults on creation, and are left alone on an update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageSpec {
    body: PackageBody,
}

impl PackageSpec {
    /// A specification that sets nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the disk allowance. The API defaults to one gigabyte.
    pub fn quota(mut self, quota: PackageQuota) -> Self {
        self.body.quota = Some(quota.to_wire());
        self
    }

    /// Sets how many domains are allowed. The API defaults to one.
    pub fn domains(mut self, limit: PackageLimit) -> Self {
        self.body.domains = Some(limit.to_wire());
        self
    }

    /// Sets how many mailboxes are allowed. The API defaults to 100.
    pub fn email_accounts(mut self, limit: PackageLimit) -> Self {
        self.body.email_accounts = Some(limit.to_wire());
        self
    }

    /// Sets how many forwarders are allowed. The API defaults to 100.
    pub fn email_forwarders(mut self, limit: PackageLimit) -> Self {
        self.body.email_forwarders = Some(limit.to_wire());
        self
    }

    /// Sets how many domain pointers are allowed. The API defaults to ten.
    pub fn domain_pointers(mut self, limit: PackageLimit) -> Self {
        self.body.domain_pointers = Some(limit.to_wire());
        self
    }

    /// Whether this specification sets anything.
    pub fn is_empty(&self) -> bool {
        !self.body.has_settings()
    }

    /// The body for a creation, which needs the name the update path takes in the URL.
    fn with_name(&self, name: &str) -> PackageBody {
        PackageBody {
            name: Some(name.to_owned()),
            ..self.body.clone()
        }
    }
}

/// Reseller package endpoints.
#[derive(Debug, Clone, Copy)]
pub struct PackagesApi<'a> {
    client: &'a Client,
}

impl<'a> PackagesApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    fn url(&self, tail: &[&str]) -> url::Url {
        let mut segments = vec!["reseller", "packages"];
        segments.extend_from_slice(tail);
        self.client.url(&segments)
    }

    /// `GET /reseller/packages` — the package names.
    ///
    /// Names only. Use [`get`](Self::get) for what a package allows.
    pub async fn list(&self) -> Result<Vec<String>> {
        let req = self.client.request(Method::GET, self.url(&[]));
        self.client.send_json(req).await
    }

    /// `GET /reseller/packages/{name}` — one package.
    pub async fn get(&self, name: &str) -> Result<Package> {
        check_path_segment("name", name)?;
        let req = self.client.request(Method::GET, self.url(&[name]));
        self.client.send_json(req).await
    }

    /// As [`get`](Self::get), with `404` mapped onto `None`.
    pub async fn try_get(&self, name: &str) -> Result<Option<Package>> {
        check_path_segment("name", name)?;
        let req = self.client.request(Method::GET, self.url(&[name]));
        self.client.send_json_opt(req).await
    }

    /// `POST /reseller/packages` — creates a package.
    ///
    /// Settings the specification leaves out take the API's own defaults, which are one
    /// gigabyte, one domain, 100 mailboxes, 100 forwarders and ten pointers.
    ///
    /// The API documents a `201` with no body.
    pub async fn create(&self, name: &str, spec: &PackageSpec) -> Result<()> {
        let req = self
            .client
            .request(Method::POST, self.url(&[]))
            .json(&spec.with_name(name))?;
        self.client.send_empty(req).await
    }

    /// `PATCH /reseller/packages/{name}` — changes a package's settings.
    ///
    /// A specification that sets nothing is refused locally rather than sent. The name
    /// cannot be changed; it addresses the package.
    pub async fn update(&self, name: &str, spec: &PackageSpec) -> Result<()> {
        check_path_segment("name", name)?;
        if spec.is_empty() {
            return Err(InvalidValue::new("spec", "would change nothing", "{}").into());
        }
        let req = self
            .client
            .request(Method::PATCH, self.url(&[name]))
            .json(&spec.body)?;
        self.client.send_empty(req).await
    }

    /// `DELETE /reseller/packages/{name}` — removes a package.
    ///
    /// Fails with [`is_validation`](crate::Error::is_validation) while any user is still
    /// on it; move them with
    /// [`set_package`](crate::api::reseller::UsersApi::set_package) first.
    pub async fn delete(&self, name: &str) -> Result<()> {
        check_path_segment("name", name)?;
        let req = self.client.request(Method::DELETE, self.url(&[name]));
        self.client.send_empty(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_stored_package_reconciles_its_nullable_settings() {
        let package: Package = serde_json::from_str(
            r#"{"name": "basic",
                "settings": {"quota_gb": 10.0, "quota_unlimited": false, "domains": 5,
                             "email_accounts": null, "email_forwarders": 100,
                             "domain_pointers": 10}}"#,
        )
        .expect("decodes");
        assert_eq!(package.settings.quota(), PackageQuota::Gigabytes(10.0));
        assert_eq!(package.settings.domain_limit(), PackageLimit::Value(5));
        // A null count is unlimited, not zero.
        assert_eq!(
            package.settings.email_account_limit(),
            PackageLimit::Unlimited
        );
        assert_eq!(package.settings.email_account_limit().value(), None);
    }

    #[test]
    fn the_unlimited_flag_wins_over_a_number_beside_it() {
        let settings: PackageSettings =
            serde_json::from_str(r#"{"quota_gb": 1.0, "quota_unlimited": true}"#).expect("decodes");
        assert_eq!(settings.quota(), PackageQuota::Unlimited);
        assert_eq!(settings.quota().gigabytes(), None);
    }

    #[test]
    fn a_null_quota_is_unlimited_even_without_the_flag() {
        let settings: PackageSettings =
            serde_json::from_str(r#"{"quota_gb": null, "quota_unlimited": false}"#)
                .expect("decodes");
        assert_eq!(settings.quota(), PackageQuota::Unlimited);
    }

    #[test]
    fn a_creation_body_is_flat_and_stringly_typed() {
        let spec = PackageSpec::new()
            .quota(PackageQuota::Gigabytes(10.0))
            .domains(PackageLimit::Value(5))
            .email_accounts(PackageLimit::Unlimited);
        let body = serde_json::to_string(&spec.with_name("basic")).expect("serializes");
        assert_eq!(
            body,
            r#"{"name":"basic","quota":"10","domains":"5","email_accounts":"unlimited"}"#
        );
    }

    #[test]
    fn a_whole_gigabyte_quota_is_written_without_a_decimal_point() {
        assert_eq!(PackageQuota::Gigabytes(1.0).to_wire(), "1");
        assert_eq!(PackageQuota::Gigabytes(10.0).to_wire(), "10");
        // A fractional one keeps its fraction rather than being rounded away.
        assert_eq!(PackageQuota::Gigabytes(1.5).to_wire(), "1.5");
        assert_eq!(PackageQuota::Unlimited.to_wire(), "unlimited");
    }

    #[test]
    fn an_update_omits_the_name_that_addresses_it() {
        let spec = PackageSpec::new().domain_pointers(PackageLimit::Value(20));
        assert_eq!(
            serde_json::to_string(&spec.body).expect("serializes"),
            r#"{"domain_pointers":"20"}"#
        );
    }

    #[test]
    fn a_specification_that_sets_nothing_knows_it() {
        assert!(PackageSpec::new().is_empty());
        assert!(!PackageSpec::new().quota(PackageQuota::Unlimited).is_empty());
    }
}
