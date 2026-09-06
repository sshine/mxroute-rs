//! The API surface, grouped by resource.
//!
//! Each group is reached from [`Client`]. The groups that live inside a domain take its
//! name where they are reached — `client.email_accounts("example.com")` — so a handle
//! cannot be carried to the wrong domain by accident.

pub mod account;
pub mod catch_all;
pub mod dns;
pub mod domains;
pub mod email_accounts;
pub mod forwarders;
pub mod pointers;
pub mod quota;
pub mod reseller;
pub mod spam;

pub use account::AccountApi;
pub use catch_all::CatchAllApi;
pub use dns::DnsApi;
pub use domains::DomainsApi;
pub use email_accounts::EmailAccountsApi;
pub use forwarders::ForwardersApi;
pub use pointers::PointersApi;
pub use quota::QuotaApi;
pub use reseller::ResellerApi;
pub use spam::{SenderListApi, SpamApi};

use crate::client::Client;

impl Client {
    /// Account-level information.
    pub fn account(&self) -> AccountApi<'_> {
        AccountApi::new(self)
    }

    /// Domain management.
    pub fn domains(&self) -> DomainsApi<'_> {
        DomainsApi::new(self)
    }

    /// Names pointed at one domain.
    pub fn pointers<'a>(&'a self, domain: &'a str) -> PointersApi<'a> {
        PointersApi::new(self, domain)
    }

    /// Mailboxes in one domain.
    pub fn email_accounts<'a>(&'a self, domain: &'a str) -> EmailAccountsApi<'a> {
        EmailAccountsApi::new(self, domain)
    }

    /// Forwarding rules in one domain.
    pub fn forwarders<'a>(&'a self, domain: &'a str) -> ForwardersApi<'a> {
        ForwardersApi::new(self, domain)
    }

    /// The records one domain needs published at its registrar.
    pub fn dns<'a>(&'a self, domain: &'a str) -> DnsApi<'a> {
        DnsApi::new(self, domain)
    }

    /// Catch-all handling for one domain.
    pub fn catch_all<'a>(&'a self, domain: &'a str) -> CatchAllApi<'a> {
        CatchAllApi::new(self, domain)
    }

    /// Spam filtering, reached through one domain but stored per account.
    pub fn spam<'a>(&'a self, domain: &'a str) -> SpamApi<'a> {
        SpamApi::new(self, domain)
    }

    /// Disk usage for the account.
    pub fn quota(&self) -> QuotaApi<'_> {
        QuotaApi::new(self)
    }

    /// User and package management, for a reseller account.
    pub fn reseller(&self) -> ResellerApi<'_> {
        ResellerApi::new(self)
    }
}
