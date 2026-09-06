//! The API surface, grouped by resource.
//!
//! Each group is reached from [`Client`]. A group scoped to one domain takes the domain
//! name when it is reached: `client.domains()` lists and creates them, while the groups
//! that live inside a domain are addressed as `client.<group>("example.com")`.

pub mod domains;

pub use domains::DomainsApi;

use crate::client::Client;

impl Client {
    /// Domain management.
    pub fn domains(&self) -> DomainsApi<'_> {
        DomainsApi::new(self)
    }
}
