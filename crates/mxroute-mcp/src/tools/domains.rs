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

#[tool_router(router = domains_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Add a domain to the account.
    #[tool(
        name = "mxroute_create_domain",
        description = "Adds a domain to the account. The TXT record from \
                       mxroute_get_verification_key has to resolve first; nothing here \
                       reports whether DNS has propagated yet.",
        annotations(
            title = "Add domain",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_domain(&self, Parameters(p): Parameters<params::Domain>) -> CallToolResult {
        match self.client.domains().create(&p.domain).await {
            Ok(created) => render::json(&created),
            Err(err) => {
                let what = format!("domain {}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_domains"))
            }
        }
    }

    /// Remove a domain and everything in it.
    #[tool(
        name = "mxroute_delete_domain",
        description = "Removes a domain from the account together with its mailboxes, \
                       forwarders and stored mail. This cannot be undone. `confirm_domain` \
                       has to repeat `domain` exactly.",
        annotations(
            title = "Delete domain",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_domain(
        &self,
        Parameters(p): Parameters<params::DeleteDomain>,
    ) -> CallToolResult {
        // The only call here that destroys stored mail, so it is the only one asked twice.
        if p.confirm_domain != p.domain {
            return render::rejected(format!(
                "confirm_domain was {:?} but domain was {:?}. Deleting a domain takes its \
                 mailboxes and stored mail with it, so the two have to match exactly.",
                p.confirm_domain, p.domain
            ));
        }

        match self.client.domains().delete(&p.domain).await {
            Ok(()) => render::confirmation(format!("Deleted {} and everything it held.", p.domain)),
            Err(err) => {
                let what = format!("domain {}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_domains"))
            }
        }
    }

    /// Turn mail for a domain on or off.
    #[tool(
        name = "mxroute_set_mail_hosting",
        description = "Turns mail hosting for a domain on or off. Turned off, the domain \
                       stays on the account but stops accepting mail.",
        annotations(
            title = "Set mail hosting",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_mail_hosting(
        &self,
        Parameters(p): Parameters<params::SetMailHosting>,
    ) -> CallToolResult {
        match self
            .client
            .domains()
            .set_mail_hosting(&p.domain, p.enabled)
            .await
        {
            Ok(()) => {
                let state = if p.enabled { "on" } else { "off" };
                render::confirmation(format!("Mail hosting for {} is now {state}.", p.domain))
            }
            Err(err) => {
                let what = format!("domain {}", p.domain);
                failed(&err, Subject::new(&what, "mxroute_list_domains"))
            }
        }
    }

    /// Point a name at a domain.
    #[tool(
        name = "mxroute_create_pointer",
        description = "Points a name at a domain, either as another name for it or as a \
                       redirect to it.",
        annotations(
            title = "Add pointer",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn create_pointer(
        &self,
        Parameters(p): Parameters<params::CreatePointer>,
    ) -> CallToolResult {
        match self
            .client
            .pointers(&p.domain)
            .create(&p.pointer, p.kind.into())
            .await
        {
            Ok(()) => render::confirmation(format!("{} now points at {}.", p.pointer, p.domain)),
            Err(err) => {
                let what = format!("pointer {} on {}", p.pointer, p.domain);
                failed(&err, Subject::new(&what, "mxroute_get_domain"))
            }
        }
    }

    /// Stop pointing a name at a domain.
    #[tool(
        name = "mxroute_delete_pointer",
        description = "Stops a name pointing at a domain. Mail addressed to the pointer stops \
                       being delivered.",
        annotations(
            title = "Delete pointer",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn delete_pointer(
        &self,
        Parameters(p): Parameters<params::DeletePointer>,
    ) -> CallToolResult {
        match self.client.pointers(&p.domain).delete(&p.pointer).await {
            Ok(()) => {
                render::confirmation(format!("{} no longer points at {}.", p.pointer, p.domain))
            }
            Err(err) => {
                let what = format!("pointer {} on {}", p.pointer, p.domain);
                failed(&err, Subject::new(&what, "mxroute_get_domain"))
            }
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
