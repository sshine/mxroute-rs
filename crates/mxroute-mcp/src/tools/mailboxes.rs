//! Mailboxes in one domain.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use mxroute::api::email_accounts::{EmailAccountPatch, NewEmailAccount};
use mxroute::{MailboxQuota, SendLimit};

use crate::error::{Subject, failed};
use crate::params;
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = mailboxes_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// The mailboxes of one domain, or one of them by name.
    #[tool(
        name = "mxroute_list_mailboxes",
        description = "Returns the mailboxes in a domain with their quota, current usage, \
                       daily send limit, messages sent today, and whether they are suspended. \
                       Given a username, returns only that mailbox. Usage figures are \
                       recomputed hourly.",
        annotations(
            title = "List mailboxes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_mailboxes(
        &self,
        Parameters(p): Parameters<params::ListMailboxes>,
    ) -> CallToolResult {
        let mailboxes = self.client.email_accounts(&p.domain);

        let Some(username) = p.username.as_deref() else {
            return match mailboxes.list().await {
                Ok(accounts) => render::page(&accounts, self.limits),
                Err(err) => {
                    let what = format!("domain {}", p.domain);
                    failed(&err, Subject::new(&what, "mxroute_list_domains"))
                }
            };
        };

        match mailboxes.get(username).await {
            Ok(account) => render::page(std::slice::from_ref(&account), self.limits),
            Err(err) => {
                let what = format!("mailbox {username}@{}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_mailboxes"))
            }
        }
    }
}

#[tool_router(router = mailboxes_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Create a mailbox.
    #[tool(
        name = "mxroute_create_mailbox",
        description = "Creates a mailbox in a domain. A quota of 0 means unlimited; omitting \
                       it takes the API's default. Returns the mailbox as stored, so the \
                       quota that was actually applied is visible.",
        annotations(
            title = "Create mailbox",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_mailbox(
        &self,
        Parameters(p): Parameters<params::CreateMailbox>,
    ) -> CallToolResult {
        let mut new = NewEmailAccount::new(&p.username, p.password.clone());
        if let Some(mb) = p.quota_mb {
            new = new.quota(MailboxQuota::from_wire(mb));
        }
        if let Some(limit) = p.send_limit {
            match SendLimit::new(limit) {
                Ok(limit) => new = new.send_limit(limit),
                Err(err) => return render::rejected(err.to_string()),
            }
        }

        let mailboxes = self.client.email_accounts(&p.domain);
        let what = format!("mailbox {}@{}", p.username, p.domain);

        if let Err(err) = mailboxes.create(&new).await {
            return failed(&err, Subject::new(&what, "mxroute_list_mailboxes"));
        }

        // The endpoint answers with an empty body, and the quota it settled on is worth
        // seeing, so this reads back rather than echoing what was asked for.
        match mailboxes.get(&p.username).await {
            Ok(account) => render::json(&account),
            Err(_) => render::confirmation(format!("Created {what}.")),
        }
    }

    /// Change a mailbox.
    #[tool(
        name = "mxroute_update_mailbox",
        description = "Changes a mailbox's password, quota or daily send limit. Anything \
                       omitted is left as it is. A quota of 0 means unlimited.",
        annotations(
            title = "Update mailbox",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn update_mailbox(
        &self,
        Parameters(p): Parameters<params::UpdateMailbox>,
    ) -> CallToolResult {
        let mut patch = EmailAccountPatch::new();
        if let Some(password) = p.password.clone() {
            patch = patch.password(password);
        }
        if let Some(mb) = p.quota_mb {
            patch = patch.quota(MailboxQuota::from_wire(mb));
        }
        if let Some(limit) = p.send_limit {
            match SendLimit::new(limit) {
                Ok(limit) => patch = patch.send_limit(limit),
                Err(err) => return render::rejected(err.to_string()),
            }
        }

        if patch.is_empty() {
            return render::rejected(
                "Nothing to change. Give at least one of password, quota_mb or send_limit.",
            );
        }

        let what = format!("mailbox {}@{}", p.username, p.domain);
        match self
            .client
            .email_accounts(&p.domain)
            .update(&p.username, &patch)
            .await
        {
            Ok(()) => render::confirmation(format!("Updated {what}.")),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_list_mailboxes")),
        }
    }

    /// Remove a mailbox.
    #[tool(
        name = "mxroute_delete_mailbox",
        description = "Removes a mailbox and the mail it holds. This cannot be undone.",
        annotations(
            title = "Delete mailbox",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_mailbox(
        &self,
        Parameters(p): Parameters<params::Mailbox>,
    ) -> CallToolResult {
        let what = format!("mailbox {}@{}", p.username, p.domain);
        match self
            .client
            .email_accounts(&p.domain)
            .delete(&p.username)
            .await
        {
            Ok(()) => render::confirmation(format!("Deleted {what} and the mail it held.")),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_list_mailboxes")),
        }
    }
}
