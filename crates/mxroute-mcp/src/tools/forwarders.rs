//! Forwarding rules in one domain.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use mxroute::Destination;
use mxroute::api::forwarders::NewForwarder;

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

#[tool_router(router = forwarders_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Forward an address somewhere else.
    #[tool(
        name = "mxroute_create_forwarder",
        description = "Forwards an address to one or more destinations. A destination may be \
                       an address, \":blackhole:\" to discard the message silently, or \
                       \":fail:\" to bounce it. Forwarding to Gmail, Yahoo or AOL turns on \
                       Expert Spam Filtering for the whole domain, which affects every other \
                       address in it.",
        annotations(
            title = "Create forwarder",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_forwarder(
        &self,
        Parameters(p): Parameters<params::CreateForwarder>,
    ) -> CallToolResult {
        let destinations: Vec<Destination> = p
            .destinations
            .iter()
            .map(|d| Destination::from(d.as_str()))
            .collect();
        if destinations.is_empty() {
            return render::rejected("A forwarder needs at least one destination.");
        }

        let new = match NewForwarder::new(&p.alias, destinations) {
            Ok(new) => new,
            Err(err) => return render::rejected(err.to_string()),
        };

        let what = format!("forwarder {}@{}", p.alias, p.domain);
        match self.client.forwarders(&p.domain).create(&new).await {
            Ok(()) => render::confirmation(format!(
                "{what} now forwards to {}.",
                p.destinations.join(", ")
            )),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_list_forwarders")),
        }
    }

    /// Stop forwarding an address.
    #[tool(
        name = "mxroute_delete_forwarder",
        description = "Removes a forwarder. Mail to the alias stops being delivered anywhere.",
        annotations(
            title = "Delete forwarder",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_forwarder(
        &self,
        Parameters(p): Parameters<params::DeleteForwarder>,
    ) -> CallToolResult {
        let what = format!("forwarder {}@{}", p.alias, p.domain);
        match self.client.forwarders(&p.domain).delete(&p.alias).await {
            Ok(()) => render::confirmation(format!("Deleted {what}.")),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_list_forwarders")),
        }
    }
}
