//! The server itself: what it holds, which tools it serves, and what it tells a client.

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::tool_handler;

use crate::render::OutputLimits;

/// Which groups of tools to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode {
    /// Serve the tools that change something.
    pub writes: bool,
    /// Serve the reseller tools.
    pub reseller: bool,
}

/// One MXroute account, served over MCP.
#[derive(Debug, Clone)]
pub struct MxrouteServer {
    pub(crate) client: mxroute::Client,
    pub(crate) limits: OutputLimits,
    mode: Mode,
    tool_router: ToolRouter<Self>,
}

impl MxrouteServer {
    pub fn new(client: mxroute::Client, mode: Mode, limits: OutputLimits) -> Self {
        Self {
            client,
            limits,
            mode,
            tool_router: Self::router(mode),
        }
    }

    /// The tools this mode serves.
    ///
    /// Composed rather than filtered: a tool that was never built cannot be reached by name
    /// either, so what `list_all` reports is exactly what can be called.
    pub fn router(mode: Mode) -> ToolRouter<Self> {
        let mut router = Self::domains_read_router();
        router.merge(Self::mailboxes_read_router());
        router.merge(Self::forwarders_read_router());
        router.merge(Self::spam_read_router());
        router.merge(Self::quota_read_router());

        if mode.writes {
            router.merge(Self::domains_write_router());
            router.merge(Self::mailboxes_write_router());
            router.merge(Self::forwarders_write_router());
            router.merge(Self::catch_all_write_router());
            router.merge(Self::spam_write_router());
        }

        if mode.reseller {
            router.merge(Self::reseller_read_router());
            if mode.writes {
                router.merge(Self::reseller_write_router());
            }
        }

        router
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MxrouteServer {
    fn get_info(&self) -> ServerInfo {
        // Named explicitly: the default is `Implementation::from_build_env`, which reads the
        // environment rmcp itself was built in, so a server that takes it announces itself as
        // "rmcp" and every server in a client's list looks the same.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(instructions(self.mode))
    }
}

/// What holds for every tool, said once here rather than in each description.
fn instructions(mode: Mode) -> String {
    let mut text = String::from(
        "This server talks to one MXroute mail server, named by MXROUTE_SERVER. Domains, \
         mailboxes, forwarders and pointers are scoped to a domain; spam sender lists and \
         quota are account-wide. MXroute allows 200 reads and 20 writes a minute and the \
         client paces itself against both, so a run of writes takes proportionally longer. \
         Quota figures are recomputed hourly, so a mailbox emptied minutes ago still reports \
         its old size.",
    );

    if !mode.writes {
        text.push_str(
            " This server was started without --allow-writes, so it serves no tool that \
             changes anything.",
        );
    }
    if !mode.reseller {
        text.push_str(" The reseller tools need --reseller and are not served.");
    }

    text
}
