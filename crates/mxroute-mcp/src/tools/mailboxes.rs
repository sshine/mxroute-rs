//! Mailboxes in one domain.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

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
