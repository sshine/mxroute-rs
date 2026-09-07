//! Domains, the names pointed at them, and the records they need published.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::{Value, json};

use crate::error::{Subject, failed};
use crate::params;
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = domains_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Every domain on the account.
    #[tool(
        name = "mxroute_list_domains",
        description = "Returns the names of every domain on the account. Details for one of \
                       them, including its pointers and catch-all, come from \
                       mxroute_get_domain.",
        annotations(
            title = "List domains",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn list_domains(&self) -> CallToolResult {
        match self.client.domains().list().await {
            Ok(domains) => render::page(&domains, self.limits),
            Err(err) => failed(&err, Subject::account("the domain list")),
        }
    }

    /// One domain, with the parts of it that live at other endpoints.
    #[tool(
        name = "mxroute_get_domain",
        description = "Returns one domain's mail-hosting and SSL flags, the names pointed at \
                       it, and its catch-all setting. Costs three requests. The DNS records \
                       the domain needs published come from mxroute_get_dns_records instead.",
        annotations(
            title = "Get domain",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_domain(&self, Parameters(p): Parameters<params::Domain>) -> CallToolResult {
        let domain = match self.client.domains().get(&p.domain).await {
            Ok(domain) => domain,
            Err(err) => {
                let what = format!("domain {}", p.domain);
                return failed(&err, Subject::new(&what, "mxroute_list_domains"));
            }
        };

        // The two extras are best effort. Both endpoints can fail for a domain that exists
        // but has never had mail turned on, and losing the whole answer to that would break
        // the tool on exactly the domain someone is trying to diagnose.
        let pointers = optional(self.client.pointers(&p.domain).list().await);
        let catch_all = optional(self.client.catch_all(&p.domain).get().await.map(
            |setting| json!({ "catch_all": setting.catch_all, "description": setting.description }),
        ));

        render::json(&json!({
            "domain": domain,
            "pointers": pointers,
            "catch_all": catch_all,
        }))
    }

    /// The records the domain needs published at its registrar.
    #[tool(
        name = "mxroute_get_dns_records",
        description = "Returns the MX, SPF, DKIM and verification records a domain needs \
                       published at its registrar. These are what MXroute expects to find; \
                       whether they actually resolve has to be checked against live DNS.",
        annotations(
            title = "Get DNS records",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_dns_records(
        &self,
        Parameters(p): Parameters<params::Domain>,
    ) -> CallToolResult {
        match self.client.dns(&p.domain).get().await {
            Ok(dns) => render::json(&dns),
            Err(err) => {
                let what = format!("domain {}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_domains"))
            }
        }
    }

    /// The account's ownership record.
    #[tool(
        name = "mxroute_get_verification_key",
        description = "Returns the account-wide TXT record that proves ownership of a domain. \
                       It is not scoped to a domain and is needed before one can be added, so \
                       it does not come from mxroute_get_dns_records.",
        annotations(
            title = "Get verification key",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_verification_key(&self) -> CallToolResult {
        match self.client.account().verification_key().await {
            Ok(key) => render::json(&key),
            Err(err) => failed(&err, Subject::account("the verification key")),
        }
    }
}

/// A section that is worth having but not worth losing the rest of the answer over.
fn optional<T: serde::Serialize>(result: mxroute::Result<T>) -> Value {
    match result {
        Ok(value) => serde_json::to_value(value).unwrap_or(Value::Null),
        Err(err) => json!({ "unavailable": err.to_string() }),
    }
}
