//! What the tools accept.
//!
//! The schemas are as tight as the API's own rules allow, because a call rejected here
//! costs nothing while one rejected by the server costs a request and a rate-limit slot.

use schemars::JsonSchema;
use serde::Deserialize;

/// A domain on the account.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Domain {
    /// Domain name, such as "example.com". No scheme, no trailing dot.
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,
}

/// The mailboxes of one domain, or one of them.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListMailboxes {
    /// Domain the mailboxes belong to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of one mailbox, without the "@domain" suffix. Given, only that mailbox
    /// is returned, and an unknown one is reported rather than silently empty.
    #[serde(default)]
    #[schemars(length(min = 1, max = 64))]
    pub username: Option<String>,
}

/// Which parts of a domain's spam configuration to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpamSection {
    /// The score at or above which a message is treated as spam.
    Threshold,
    /// Senders whose mail is never treated as spam.
    Whitelist,
    /// Senders whose mail is always treated as spam.
    Blacklist,
}

impl SpamSection {
    pub const ALL: [Self; 3] = [Self::Threshold, Self::Whitelist, Self::Blacklist];
}

/// A domain's spam configuration.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SpamSettings {
    /// Domain to reach the settings through, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Which sections to fetch, each costing one request. Omitted, all three are fetched.
    #[serde(default)]
    pub include: Option<Vec<SpamSection>>,
}

impl SpamSettings {
    /// The sections to fetch, with the empty selection meaning all of them rather than none.
    pub fn sections(&self) -> Vec<SpamSection> {
        match &self.include {
            Some(sections) if !sections.is_empty() => sections.clone(),
            _ => SpamSection::ALL.to_vec(),
        }
    }
}

/// A domain to remove, named twice.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteDomain {
    /// Domain to remove, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// The same domain again, spelled exactly as above.
    #[schemars(length(min = 1, max = 253))]
    pub confirm_domain: String,
}

/// Whether a domain accepts mail.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetMailHosting {
    /// Domain to change, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// True to accept mail for the domain, false to stop.
    pub enabled: bool,
}

/// How a pointer resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PointerKind {
    /// Treat the pointer as another name for the target.
    Alias,
    /// Redirect the pointer to the target.
    Redirect,
}

impl From<PointerKind> for mxroute::api::pointers::PointerKind {
    fn from(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Alias => Self::Alias,
            PointerKind::Redirect => Self::Redirect,
        }
    }
}

/// A name to point at a domain.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreatePointer {
    /// Domain to point at, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Name to point at it, such as "example.net".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub pointer: String,

    /// Whether the pointer is another name for the domain or a redirect to it.
    pub kind: PointerKind,
}

/// A name to stop pointing at a domain.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeletePointer {
    /// Domain the name points at, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Name to remove, such as "example.net".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub pointer: String,
}

/// A mailbox to create.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateMailbox {
    /// Domain the mailbox belongs to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of the address, without the "@domain" suffix.
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[a-z0-9][a-z0-9._-]*$"))]
    pub username: String,

    /// The mailbox password. MXroute requires at least eight characters.
    #[schemars(length(min = 8, max = 128))]
    pub password: String,

    /// Mailbox size in megabytes, where 0 means unlimited. Omitted, the API's default applies.
    pub quota_mb: Option<u32>,

    /// Messages the mailbox may send per day, up to 9600.
    #[schemars(range(min = 0, max = 9600))]
    pub send_limit: Option<u32>,
}

/// Changes to an existing mailbox.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateMailbox {
    /// Domain the mailbox belongs to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of the address, without the "@domain" suffix.
    #[schemars(length(min = 1, max = 64))]
    pub username: String,

    /// A new password. Omitted, the current one stands.
    #[schemars(length(min = 8, max = 128))]
    pub password: Option<String>,

    /// A new size in megabytes, where 0 means unlimited. Omitted, the current one stands.
    pub quota_mb: Option<u32>,

    /// A new daily send limit, up to 9600. Omitted, the current one stands.
    #[schemars(range(min = 0, max = 9600))]
    pub send_limit: Option<u32>,
}

/// One mailbox, by name.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Mailbox {
    /// Domain the mailbox belongs to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of the address, without the "@domain" suffix.
    #[schemars(length(min = 1, max = 64))]
    pub username: String,
}

/// A forwarding rule to create.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateForwarder {
    /// Domain the alias belongs to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of the address to forward from, without the "@domain" suffix.
    #[schemars(length(min = 1, max = 64))]
    pub alias: String,

    /// Where the mail goes: an address, ":blackhole:" to discard it, or ":fail:" to bounce it.
    #[schemars(length(min = 1, max = 20))]
    pub destinations: Vec<String>,
}

/// A forwarding rule to remove.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteForwarder {
    /// Domain the alias belongs to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of the address to stop forwarding, without the "@domain" suffix.
    #[schemars(length(min = 1, max = 64))]
    pub alias: String,
}

/// What happens to mail for an address that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CatchAllMode {
    /// Reject the message at delivery.
    Fail,
    /// Accept the message and discard it.
    Blackhole,
    /// Deliver it to a named address.
    Address,
}

/// A domain's catch-all handling.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetCatchAll {
    /// Domain to change, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// What to do with mail for an unknown address.
    pub mode: CatchAllMode,

    /// Where to deliver it. Required for mode "address" and meaningless otherwise.
    #[schemars(length(min = 3, max = 254))]
    pub address: Option<String>,
}

impl SetCatchAll {
    /// The library's catch-all, which cannot represent an address without one.
    pub fn catch_all(&self) -> Result<mxroute::api::catch_all::CatchAll, &'static str> {
        use mxroute::api::catch_all::CatchAll;

        match (self.mode, self.address.as_deref()) {
            (CatchAllMode::Fail, _) => Ok(CatchAll::Fail),
            (CatchAllMode::Blackhole, _) => Ok(CatchAll::Blackhole),
            (CatchAllMode::Address, Some(address)) => Ok(CatchAll::Address(address.to_owned())),
            (CatchAllMode::Address, None) => {
                Err("mode \"address\" needs an address to deliver to.")
            }
        }
    }
}

/// A domain's spam threshold.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetSpamScore {
    /// Domain to change, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// The score at or above which mail is treated as spam and deleted, from 1 to 50. A
    /// lower number catches more spam and more legitimate mail with it.
    #[schemars(range(min = 1, max = 50))]
    pub score: u8,
}

/// Which of the two account-wide sender lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SenderList {
    /// Senders whose mail is never treated as spam.
    Whitelist,
    /// Senders whose mail is always treated as spam.
    Blacklist,
}

/// One entry on one of the sender lists.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SpamSender {
    /// Domain to reach the list through, such as "example.com". The list itself is
    /// account-wide, so which domain is used does not change what is affected.
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Which list to change.
    pub list: SenderList,

    /// An address such as "sender@example.net", or a domain such as "example.net".
    #[schemars(length(min = 1, max = 254))]
    pub sender: String,
}

/// The reseller's users, or one of them.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListResellerUsers {
    /// One username. Given, only that user is returned.
    #[serde(default)]
    #[schemars(length(min = 1, max = 10))]
    pub username: Option<String>,
}

/// The reseller's packages, or one of them.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListResellerPackages {
    /// One package name. Given, only that package is returned.
    #[serde(default)]
    #[schemars(length(min = 1, max = 64))]
    pub name: Option<String>,
}

/// A user to create under the reseller account.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateResellerUser {
    /// Username, one to ten characters of lowercase letters, digits and underscores. It
    /// cannot be changed afterwards.
    #[schemars(length(min = 1, max = 10), regex(pattern = r"^[a-z0-9_]+$"))]
    pub username: String,

    /// Contact address for the user.
    #[schemars(length(min = 3, max = 254))]
    pub email: String,

    /// The user's password. MXroute requires at least eight characters.
    #[schemars(length(min = 8, max = 128))]
    pub password: String,

    /// Name of an existing package, which sets the user's limits.
    #[schemars(length(min = 1, max = 64))]
    pub package: String,
}

/// Changes to an existing reseller user.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateResellerUser {
    /// The user to change.
    #[schemars(length(min = 1, max = 10))]
    pub username: String,

    /// A new password. Omitted, the current one stands.
    #[schemars(length(min = 8, max = 128))]
    pub password: Option<String>,

    /// A new storage allowance in megabytes, unlike a package's, which is in gigabytes.
    pub quota_mb: Option<u64>,

    /// True to lift the user's storage limit entirely. Overrides quota_mb.
    pub quota_unlimited: Option<bool>,
}

/// One reseller user, by name.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ResellerUser {
    /// The user to address.
    #[schemars(length(min = 1, max = 10))]
    pub username: String,
}

/// A reseller user to remove, named twice.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteResellerUser {
    /// The user to remove.
    #[schemars(length(min = 1, max = 10))]
    pub username: String,

    /// The same username again, spelled exactly as above.
    #[schemars(length(min = 1, max = 10))]
    pub confirm_username: String,
}

/// Whether a reseller user is suspended.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SuspendResellerUser {
    /// The user to change.
    #[schemars(length(min = 1, max = 10))]
    pub username: String,

    /// True to suspend the user, false to let them back in.
    pub suspended: bool,
}

/// A reseller user's package.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetResellerUserPackage {
    /// The user to change.
    #[schemars(length(min = 1, max = 10))]
    pub username: String,

    /// Name of an existing package to move them onto.
    #[schemars(length(min = 1, max = 64))]
    pub package: String,
}

/// The limits a package grants.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ResellerPackage {
    /// Package name.
    #[schemars(length(min = 1, max = 64))]
    pub name: String,

    /// Storage allowance in gigabytes, unlike a user's, which is in megabytes. Omitted on
    /// an update, the current value stands.
    pub quota_gb: Option<f64>,

    /// True for no storage limit. Overrides quota_gb.
    pub quota_unlimited: Option<bool>,

    /// How many domains the package allows. Null for no limit.
    pub domains: Option<u32>,

    /// True for no limit on domains. Overrides domains.
    pub domains_unlimited: Option<bool>,

    /// How many mailboxes the package allows. Null for no limit.
    pub email_accounts: Option<u32>,

    /// True for no limit on mailboxes. Overrides email_accounts.
    pub email_accounts_unlimited: Option<bool>,

    /// How many forwarders the package allows. Null for no limit.
    pub email_forwarders: Option<u32>,

    /// True for no limit on forwarders. Overrides email_forwarders.
    pub email_forwarders_unlimited: Option<bool>,

    /// How many domain pointers the package allows. Null for no limit.
    pub domain_pointers: Option<u32>,

    /// True for no limit on domain pointers. Overrides domain_pointers.
    pub domain_pointers_unlimited: Option<bool>,
}

/// A package to remove.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteResellerPackage {
    /// Package name.
    #[schemars(length(min = 1, max = 64))]
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catch_all_address_without_an_address_is_refused() {
        let params = SetCatchAll {
            domain: "example.com".to_owned(),
            mode: CatchAllMode::Address,
            address: None,
        };
        assert!(params.catch_all().is_err());
    }

    #[test]
    fn the_other_catch_all_modes_ignore_a_stray_address() {
        for mode in [CatchAllMode::Fail, CatchAllMode::Blackhole] {
            let params = SetCatchAll {
                domain: "example.com".to_owned(),
                mode,
                address: Some("in@example.com".to_owned()),
            };
            assert!(params.catch_all().is_ok());
        }
    }

    #[test]
    fn asking_for_no_spam_section_asks_for_all_of_them() {
        let all = SpamSection::ALL.to_vec();
        for include in [None, Some(Vec::new())] {
            let params = SpamSettings {
                domain: "example.com".to_owned(),
                include,
            };
            assert_eq!(params.sections(), all);
        }
    }

    #[test]
    fn a_named_spam_section_is_the_only_one_fetched() {
        let params = SpamSettings {
            domain: "example.com".to_owned(),
            include: Some(vec![SpamSection::Blacklist]),
        };
        assert_eq!(params.sections(), vec![SpamSection::Blacklist]);
    }
}
