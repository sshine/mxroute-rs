//! Disk usage: `/quota` and `/quota/email`.
//!
//! Both endpoints report figures refreshed hourly, so a mailbox emptied a minute ago still
//! shows its old size.
//!
//! These two are also the only endpoints in the API that answer without the
//! `{"success": …, "data": …}` envelope everything else uses: their fields sit at the top
//! level of the body.

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

/// Disk used, broken down by what is using it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[non_exhaustive]
pub struct UsageBreakdown {
    /// Bytes held by mail.
    #[serde(default)]
    pub email: u64,
    /// Bytes held by web content.
    #[serde(default)]
    pub web: u64,
    /// Bytes held by databases.
    #[serde(default)]
    pub databases: u64,
    /// Bytes held by backups.
    #[serde(default)]
    pub backups: u64,
    /// Bytes held by anything else.
    #[serde(default)]
    pub other: u64,
}

/// How long an over-quota account has before something is done about it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct GracePeriod {
    /// Days left.
    pub days_remaining: i64,
    /// When the period ends, as the API spells it.
    pub deadline: String,
}

/// The account's disk usage against its allowance.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Quota {
    /// The DirectAdmin username these figures are for.
    pub username: String,
    /// Bytes used in total.
    pub total_used: u64,
    /// The allowance in bytes, where `0` means unlimited.
    ///
    /// Left as the wire value rather than an enum: unlike a mailbox quota this is only
    /// ever read, so [`limit_bytes`](Self::limit_bytes) is the only place the zero has to
    /// be interpreted.
    pub total_limit: u64,
    /// Usage as a percentage of the allowance.
    pub percent_used: f64,
    /// What the space is going to.
    #[serde(default)]
    pub breakdown: UsageBreakdown,
    /// Present only while the account is over its allowance.
    #[serde(default)]
    pub grace_period: Option<GracePeriod>,
    /// When these figures were last recomputed.
    pub updated_at: DateTime<Utc>,
}

impl Quota {
    /// The allowance in bytes, or `None` when there is no limit.
    pub fn limit_bytes(&self) -> Option<u64> {
        match self.total_limit {
            0 => None,
            limit => Some(limit),
        }
    }

    /// Whether the account is over its allowance.
    pub fn is_over_quota(&self) -> bool {
        self.limit_bytes()
            .is_some_and(|limit| self.total_used > limit)
    }
}

/// One mailbox's share of the disk.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct MailboxUsage {
    /// The full address.
    pub email_address: String,
    /// Bytes held.
    pub size_bytes: u64,
    /// When the figure was last recomputed.
    pub updated_at: DateTime<Utc>,
}

/// Per-mailbox disk usage for the account.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct EmailUsage {
    /// The DirectAdmin username these figures are for.
    pub username: String,
    /// Every mailbox, largest first.
    #[serde(default)]
    pub accounts: Vec<MailboxUsage>,
}

/// Quota endpoints.
#[derive(Debug, Clone, Copy)]
pub struct QuotaApi<'a> {
    client: &'a Client,
}

impl<'a> QuotaApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// `GET /quota` — the account's disk usage.
    pub async fn account(&self) -> Result<Quota> {
        let url = self.client.url(&["quota"]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json_bare(req).await
    }

    /// `GET /quota/email` — disk usage per mailbox, largest first.
    pub async fn email(&self) -> Result<EmailUsage> {
        let url = self.client.url(&["quota", "email"]);
        let req = self.client.request(Method::GET, url);
        self.client.send_json_bare(req).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_quota_decodes_from_a_body_with_no_envelope() {
        let quota: Quota = serde_json::from_str(
            r#"{"username": "johndoe", "total_used": 5368709120,
                "total_limit": 10737418240, "percent_used": 50.0,
                "breakdown": {"email": 5368709120, "web": 0, "databases": 0,
                              "backups": 0, "other": 0},
                "grace_period": null, "updated_at": "2026-09-06T04:00:00Z"}"#,
        )
        .expect("decodes");
        assert_eq!(quota.limit_bytes(), Some(10_737_418_240));
        assert!(!quota.is_over_quota());
        assert_eq!(quota.grace_period, None);
        assert_eq!(quota.breakdown.email, 5_368_709_120);
    }

    #[test]
    fn a_zero_limit_reads_as_unlimited_and_can_never_be_exceeded() {
        let quota: Quota = serde_json::from_str(
            r#"{"username": "johndoe", "total_used": 99999999, "total_limit": 0,
                "percent_used": 0.0, "updated_at": "2026-09-06T04:00:00Z"}"#,
        )
        .expect("breakdown and grace period are optional");
        assert_eq!(quota.limit_bytes(), None);
        assert!(!quota.is_over_quota());
        assert_eq!(quota.breakdown, UsageBreakdown::default());
    }

    #[test]
    fn an_account_past_its_limit_reports_its_grace_period() {
        let quota: Quota = serde_json::from_str(
            r#"{"username": "johndoe", "total_used": 20000, "total_limit": 10000,
                "percent_used": 200.0,
                "grace_period": {"days_remaining": 7, "deadline": "2026-09-13"},
                "updated_at": "2026-09-06T04:00:00Z"}"#,
        )
        .expect("decodes");
        assert!(quota.is_over_quota());
        assert_eq!(quota.grace_period.map(|g| g.days_remaining), Some(7));
    }

    #[test]
    fn per_mailbox_usage_decodes() {
        let usage: EmailUsage = serde_json::from_str(
            r#"{"username": "johndoe",
                "accounts": [{"email_address": "sales@example.com", "size_bytes": 1024,
                              "updated_at": "2026-09-06T04:00:00Z"}]}"#,
        )
        .expect("decodes");
        assert_eq!(usage.accounts.len(), 1);
        assert_eq!(usage.accounts[0].size_bytes, 1024);
    }
}
