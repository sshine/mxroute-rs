//! Forwarding rules in one domain.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use crate::error::{Subject, failed};
use crate::params;
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = forwarders_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// The forwarding rules of one domain.
    #[tool(
        name = "mxroute_list_forwarders",
        description = "Returns the forwarders in a domain: each alias and the addresses its \
                       mail is copied to. A destination of :blackhole: discards the message \
                       and :fail: bounces it.",
        annotations(
            title = "List forwarders",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_forwarders(
        &self,
        Parameters(p): Parameters<params::Domain>,
    ) -> CallToolResult {
        match self.client.forwarders(&p.domain).list().await {
            Ok(forwarders) => render::page(&forwarders, self.limits),
            Err(err) => {
                let what = format!("domain {}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_domains"))
            }
        }
    }
}
