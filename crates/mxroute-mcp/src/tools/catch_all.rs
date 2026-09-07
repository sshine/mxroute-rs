//! What happens to mail for an address that does not exist.
//!
//! There is no read tool here: `mxroute_get_domain` already answers with the setting, and a
//! second tool for one field would be a near-duplicate of it.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};

use crate::error::{Subject, failed};
use crate::params;
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = catch_all_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Decide what happens to mail for an unknown address.
    #[tool(
        name = "mxroute_set_catch_all",
        description = "Sets what happens to mail addressed to a name that does not exist in \
                       the domain: \"fail\" bounces it, \"blackhole\" accepts and discards it, \
                       and \"address\" delivers it to the address given. A catch-all that \
                       accepts everything also accepts everything a spammer guesses. The \
                       current setting comes from mxroute_get_domain.",
        annotations(
            title = "Set catch-all",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_catch_all(
        &self,
        Parameters(p): Parameters<params::SetCatchAll>,
    ) -> CallToolResult {
        let catch_all = match p.catch_all() {
            Ok(catch_all) => catch_all,
            Err(reason) => return render::rejected(reason),
        };

        let what = format!("the catch-all for {}", p.domain);
        match self.client.catch_all(&p.domain).set(&catch_all).await {
            Ok(()) => render::confirmation(match &p.address {
                Some(address) if matches!(p.mode, params::CatchAllMode::Address) => {
                    format!(
                        "Mail to an unknown address at {} now goes to {address}.",
                        p.domain
                    )
                }
                _ => format!(
                    "Mail to an unknown address at {} is now handled by {:?}.",
                    p.domain, p.mode
                ),
            }),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_get_domain")),
        }
    }
}
