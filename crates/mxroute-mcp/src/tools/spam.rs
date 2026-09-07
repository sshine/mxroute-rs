//! Spam filtering, reached through a domain but stored per account.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::{Map, Value, json};

use crate::error::{Subject, failed};
use crate::params::{self, SpamSection};
use crate::render;
use crate::server::MxrouteServer;

#[tool_router(router = spam_read_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// A domain's spam threshold and the account's two sender lists.
    #[tool(
        name = "mxroute_get_spam_settings",
        description = "Returns the score at or above which mail is treated as spam, plus the \
                       sender whitelist and blacklist. The two lists are stored per account, \
                       not per domain: an entry read through one domain applies to every \
                       domain on the account. Each section costs one request.",
        annotations(
            title = "Get spam settings",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_spam_settings(
        &self,
        Parameters(p): Parameters<params::SpamSettings>,
    ) -> CallToolResult {
        let spam = self.client.spam(&p.domain);
        let what = format!("domain {}", p.domain);
        let mut out = Map::new();

        for section in p.sections() {
            let value = match section {
                SpamSection::Threshold => match spam.settings().await {
                    Ok(settings) => json!(settings.high_score),
                    Err(err) => return failed(&err, Subject::new(&what, "mxroute_list_domains")),
                },
                SpamSection::Whitelist => match spam.whitelist().list().await {
                    Ok(entries) => json!(entries),
                    Err(err) => return failed(&err, Subject::new(&what, "mxroute_list_domains")),
                },
                SpamSection::Blacklist => match spam.blacklist().list().await {
                    Ok(entries) => json!(entries),
                    Err(err) => return failed(&err, Subject::new(&what, "mxroute_list_domains")),
                },
            };
            out.insert(name(section).to_owned(), value);
        }

        render::json(&Value::Object(out))
    }
}

fn name(section: SpamSection) -> &'static str {
    match section {
        SpamSection::Threshold => "high_score",
        SpamSection::Whitelist => "whitelist",
        SpamSection::Blacklist => "blacklist",
    }
}
