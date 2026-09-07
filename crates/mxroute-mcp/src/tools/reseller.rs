//! Users and packages, for an account that resells.
//!
//! Served only with `--reseller`. Most accounts do not have these endpoints at all, and
//! twelve tools that answer 403 are twelve tools' worth of schema in every request.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use mxroute::api::reseller::packages::{PackageLimit, PackageQuota, PackageSpec};
use mxroute::api::reseller::users::{NewResellerUser, ResellerUserPatch};

use crate::error::{Subject, failed};
use crate::params;
use crate::render;
use crate::server::MxrouteServer;

const USERS: &str = "mxroute_list_reseller_users";
const PACKAGES: &str = "mxroute_list_reseller_packages";

#[tool_router(router = reseller_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// The users under this reseller account.
    #[tool(
        name = "mxroute_list_reseller_users",
        description = "Returns the users under this reseller account. Given a username, \
                       returns that user's package, quota and suspension state instead of \
                       just the names.",
        annotations(
            title = "List reseller users",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_reseller_users(
        &self,
        Parameters(p): Parameters<params::ListResellerUsers>,
    ) -> CallToolResult {
        let users = self.client.reseller().users();

        let Some(username) = p.username.as_deref() else {
            return match users.list().await {
                Ok(names) => render::page(&names, self.limits),
                Err(err) => failed(&err, Subject::account("the reseller user list")),
            };
        };

        match users.get(username).await {
            Ok(user) => render::json(&user),
            Err(err) => {
                let what = format!("reseller user {username}");
                failed(&err, Subject::new(&what, USERS))
            }
        }
    }

    /// The packages this reseller account offers.
    #[tool(
        name = "mxroute_list_reseller_packages",
        description = "Returns the packages this reseller account offers. Given a name, \
                       returns that package's storage allowance and its caps on domains, \
                       mailboxes, forwarders and pointers.",
        annotations(
            title = "List reseller packages",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_reseller_packages(
        &self,
        Parameters(p): Parameters<params::ListResellerPackages>,
    ) -> CallToolResult {
        let packages = self.client.reseller().packages();

        let Some(name) = p.name.as_deref() else {
            return match packages.list().await {
                Ok(names) => render::page(&names, self.limits),
                Err(err) => failed(&err, Subject::account("the reseller package list")),
            };
        };

        match packages.get(name).await {
            Ok(package) => render::json(&package),
            Err(err) => {
                let what = format!("reseller package {name}");
                failed(&err, Subject::new(&what, PACKAGES))
            }
        }
    }
}

#[tool_router(router = reseller_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Add a user under this reseller account.
    #[tool(
        name = "mxroute_create_reseller_user",
        description = "Creates a user under this reseller account on an existing package. \
                       The username is one to ten characters of lowercase letters, digits \
                       and underscores, and cannot be changed afterwards.",
        annotations(
            title = "Create reseller user",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_reseller_user(
        &self,
        Parameters(p): Parameters<params::CreateResellerUser>,
    ) -> CallToolResult {
        let new = match NewResellerUser::new(&p.username, &p.email, p.password.clone(), &p.package)
        {
            Ok(new) => new,
            Err(err) => return render::rejected(err.to_string()),
        };

        let what = format!("reseller user {}", p.username);
        match self.client.reseller().users().create(&new).await {
            Ok(()) => render::confirmation(format!("Created {what} on package {}.", p.package)),
            Err(err) => failed(&err, Subject::new(&what, USERS)),
        }
    }

    /// Change a user's password or storage allowance.
    #[tool(
        name = "mxroute_update_reseller_user",
        description = "Changes a reseller user's password or storage allowance. The quota is \
                       in megabytes here, unlike a package's, which is in gigabytes. Moving \
                       a user to another package is mxroute_set_reseller_user_package.",
        annotations(
            title = "Update reseller user",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn update_reseller_user(
        &self,
        Parameters(p): Parameters<params::UpdateResellerUser>,
    ) -> CallToolResult {
        let mut patch = ResellerUserPatch::new();
        if let Some(password) = p.password.clone() {
            patch = patch.password(password);
        }
        if p.quota_unlimited == Some(true) {
            patch = patch.unlimited_quota();
        } else if let Some(mb) = p.quota_mb {
            patch = patch.quota_megabytes(mb);
        }

        if patch.is_empty() {
            return render::rejected(
                "Nothing to change. Give at least one of password, quota_mb or quota_unlimited.",
            );
        }

        let what = format!("reseller user {}", p.username);
        match self
            .client
            .reseller()
            .users()
            .update(&p.username, &patch)
            .await
        {
            Ok(()) => render::confirmation(format!("Updated {what}.")),
            Err(err) => failed(&err, Subject::new(&what, USERS)),
        }
    }

    /// Remove a user and everything they hold.
    #[tool(
        name = "mxroute_delete_reseller_user",
        description = "Removes a reseller user together with their domains, mailboxes and \
                       stored mail. This cannot be undone. `confirm_username` has to repeat \
                       `username` exactly. To stop access without losing the data, suspend \
                       them with mxroute_set_reseller_user_suspended instead.",
        annotations(
            title = "Delete reseller user",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_reseller_user(
        &self,
        Parameters(p): Parameters<params::DeleteResellerUser>,
    ) -> CallToolResult {
        if p.confirm_username != p.username {
            return render::rejected(format!(
                "confirm_username was {:?} but username was {:?}. Deleting a user takes their \
                 domains and stored mail with it, so the two have to match exactly.",
                p.confirm_username, p.username
            ));
        }

        let what = format!("reseller user {}", p.username);
        match self.client.reseller().users().delete(&p.username).await {
            Ok(()) => render::confirmation(format!("Deleted {what} and everything they held.")),
            Err(err) => failed(&err, Subject::new(&what, USERS)),
        }
    }

    /// Suspend a user, or let them back in.
    #[tool(
        name = "mxroute_set_reseller_user_suspended",
        description = "Suspends a reseller user or lifts the suspension. A suspended user \
                       keeps their domains, mailboxes and stored mail but cannot use them.",
        annotations(
            title = "Suspend reseller user",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_reseller_user_suspended(
        &self,
        Parameters(p): Parameters<params::SuspendResellerUser>,
    ) -> CallToolResult {
        let users = self.client.reseller().users();
        let result = if p.suspended {
            users.suspend(&p.username).await
        } else {
            users.unsuspend(&p.username).await
        };

        let what = format!("reseller user {}", p.username);
        match result {
            Ok(()) => render::confirmation(if p.suspended {
                format!("Suspended {what}; their data is untouched.")
            } else {
                format!("Lifted the suspension on {what}.")
            }),
            Err(err) => failed(&err, Subject::new(&what, USERS)),
        }
    }

    /// Move a user onto another package.
    #[tool(
        name = "mxroute_set_reseller_user_package",
        description = "Moves a reseller user onto an existing package, which replaces their \
                       limits with that package's. Returns the user as stored afterwards.",
        annotations(
            title = "Set reseller user package",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_reseller_user_package(
        &self,
        Parameters(p): Parameters<params::SetResellerUserPackage>,
    ) -> CallToolResult {
        let what = format!("reseller user {}", p.username);
        match self
            .client
            .reseller()
            .users()
            .set_package(&p.username, &p.package)
            .await
        {
            Ok(user) => render::json(&user),
            Err(err) => failed(&err, Subject::new(&what, USERS)),
        }
    }

    /// Add a package.
    #[tool(
        name = "mxroute_create_reseller_package",
        description = "Creates a package. The storage allowance is in gigabytes here, unlike \
                       a user's, which is in megabytes. Each limit is either a number or its \
                       matching _unlimited flag; the flag wins.",
        annotations(
            title = "Create reseller package",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_reseller_package(
        &self,
        Parameters(p): Parameters<params::ResellerPackage>,
    ) -> CallToolResult {
        let spec = spec(&p);
        if spec.is_empty() {
            return render::rejected("A package needs at least one limit or allowance.");
        }

        let what = format!("reseller package {}", p.name);
        match self
            .client
            .reseller()
            .packages()
            .create(&p.name, &spec)
            .await
        {
            Ok(()) => render::confirmation(format!("Created {what}.")),
            Err(err) => failed(&err, Subject::new(&what, PACKAGES)),
        }
    }

    /// Change a package's limits.
    #[tool(
        name = "mxroute_update_reseller_package",
        description = "Changes a package's storage allowance or limits. Anything omitted is \
                       left as it is. Every user on the package is affected.",
        annotations(
            title = "Update reseller package",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn update_reseller_package(
        &self,
        Parameters(p): Parameters<params::ResellerPackage>,
    ) -> CallToolResult {
        let spec = spec(&p);
        if spec.is_empty() {
            return render::rejected("Nothing to change. Give at least one allowance or limit.");
        }

        let what = format!("reseller package {}", p.name);
        match self
            .client
            .reseller()
            .packages()
            .update(&p.name, &spec)
            .await
        {
            Ok(()) => render::confirmation(format!("Updated {what}.")),
            Err(err) => failed(&err, Subject::new(&what, PACKAGES)),
        }
    }

    /// Remove a package.
    #[tool(
        name = "mxroute_delete_reseller_package",
        description = "Removes a package. Users still on it are left without one, so move \
                       them first with mxroute_set_reseller_user_package.",
        annotations(
            title = "Delete reseller package",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_reseller_package(
        &self,
        Parameters(p): Parameters<params::DeleteResellerPackage>,
    ) -> CallToolResult {
        let what = format!("reseller package {}", p.name);
        match self.client.reseller().packages().delete(&p.name).await {
            Ok(()) => render::confirmation(format!("Deleted {what}.")),
            Err(err) => failed(&err, Subject::new(&what, PACKAGES)),
        }
    }
}

/// Fold the flat parameters into the library's spec, letting each flag win over its number.
fn spec(p: &params::ResellerPackage) -> PackageSpec {
    let mut spec = PackageSpec::new();

    if p.quota_unlimited == Some(true) {
        spec = spec.quota(PackageQuota::Unlimited);
    } else if let Some(gb) = p.quota_gb {
        spec = spec.quota(PackageQuota::Gigabytes(gb));
    }

    if let Some(limit) = limit(p.domains, p.domains_unlimited) {
        spec = spec.domains(limit);
    }
    if let Some(limit) = limit(p.email_accounts, p.email_accounts_unlimited) {
        spec = spec.email_accounts(limit);
    }
    if let Some(limit) = limit(p.email_forwarders, p.email_forwarders_unlimited) {
        spec = spec.email_forwarders(limit);
    }
    if let Some(limit) = limit(p.domain_pointers, p.domain_pointers_unlimited) {
        spec = spec.domain_pointers(limit);
    }

    spec
}

fn limit(value: Option<u32>, unlimited: Option<bool>) -> Option<PackageLimit> {
    match (unlimited, value) {
        (Some(true), _) => Some(PackageLimit::Unlimited),
        (_, Some(value)) => Some(PackageLimit::Value(value)),
        _ => None,
    }
}
