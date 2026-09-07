//! Disk usage, for the account and for each mailbox.

use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use crate::error::{Subject, failed};
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = quota_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// What the account is using, in total and per mailbox.
    #[tool(
        name = "mxroute_get_quota",
        description = "Returns the account's storage limit and what it is using, broken down \
                       by category, along with the size of every mailbox. Figures are \
                       recomputed hourly, so a mailbox emptied minutes ago still reports its \
                       old size. Costs two requests.",
        annotations(
            title = "Get quota",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_quota(&self) -> CallToolResult {
        let quota = self.client.quota();

        let account = match quota.account().await {
            Ok(account) => account,
            Err(err) => return failed(&err, Subject::account("the account quota")),
        };
        let email = match quota.email().await {
            Ok(email) => email,
            Err(err) => return failed(&err, Subject::account("the per-mailbox usage")),
        };

        render::json(&json!({
            "account": account,
            "limit_bytes": account.limit_bytes(),
            "over_quota": account.is_over_quota(),
            "email": email,
        }))
    }
}
