//! Reseller management: `/reseller`.
//!
//! Only a reseller account may use any of this; every endpoint here answers `403`
//! ([`Error::is_forbidden`](crate::Error::is_forbidden)) otherwise.
//!
//! Reached as `client.reseller().users()` and `client.reseller().packages()`.

pub mod packages;
pub mod users;

pub use packages::PackagesApi;
pub use users::UsersApi;

use crate::client::Client;

/// Reseller endpoints.
#[derive(Debug, Clone, Copy)]
pub struct ResellerApi<'a> {
    client: &'a Client,
}

impl<'a> ResellerApi<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// The users under this reseller.
    pub fn users(&self) -> UsersApi<'a> {
        UsersApi::new(self.client)
    }

    /// The packages a user can be assigned.
    pub fn packages(&self) -> PackagesApi<'a> {
        PackagesApi::new(self.client)
    }
}
