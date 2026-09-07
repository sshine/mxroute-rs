//! Spam filtering, reached through a domain but stored per account.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::{Map, Value, json};

use mxroute::api::spam::SenderListApi;
use mxroute::{SpamEntry, SpamScore};

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

#[tool_router(router = spam_write_router, vis = "pub(crate)")]
impl MxrouteServer {
    /// Set the score at which mail is treated as spam.
    #[tool(
        name = "mxroute_set_spam_score",
        description = "Sets the score at or above which a domain's mail is treated as spam \
                       and deleted, from 1 to 50. A lower number catches more spam and more \
                       legitimate mail with it.",
        annotations(
            title = "Set spam score",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn set_spam_score(
        &self,
        Parameters(p): Parameters<params::SetSpamScore>,
    ) -> CallToolResult {
        let score = match SpamScore::new(p.score) {
            Ok(score) => score,
            Err(err) => return render::rejected(err.to_string()),
        };

        let what = format!("the spam score for {}", p.domain);
        match self.client.spam(&p.domain).set_high_score(score).await {
            Ok(()) => render::confirmation(format!(
                "Mail scoring {} or above on {} is now treated as spam.",
                p.score, p.domain
            )),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_get_spam_settings")),
        }
    }

    /// Add a sender to the whitelist or the blacklist.
    #[tool(
        name = "mxroute_add_spam_sender",
        description = "Adds an address or a domain to the account's sender whitelist or \
                       blacklist. Both lists are account-wide: the entry applies to every \
                       domain on the account, not only the one it was added through.",
        annotations(
            title = "Add spam sender",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn add_spam_sender(
        &self,
        Parameters(p): Parameters<params::SpamSender>,
    ) -> CallToolResult {
        let entry = match SpamEntry::new(p.sender.clone()) {
            Ok(entry) => entry,
            Err(err) => return render::rejected(err.to_string()),
        };

        let what = format!("the {} on this account", list_name(p.list));
        match self.list(&p.domain, p.list).add(&entry).await {
            Ok(()) => render::confirmation(format!(
                "{} is on the account-wide {}, for every domain.",
                p.sender,
                list_name(p.list)
            )),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_get_spam_settings")),
        }
    }

    /// Take a sender off the whitelist or the blacklist.
    #[tool(
        name = "mxroute_remove_spam_sender",
        description = "Removes an address or a domain from the account's sender whitelist or \
                       blacklist. Both lists are account-wide, so this removes it for every \
                       domain on the account, not only the one it is removed through.",
        annotations(
            title = "Remove spam sender",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn remove_spam_sender(
        &self,
        Parameters(p): Parameters<params::SpamSender>,
    ) -> CallToolResult {
        let entry = match SpamEntry::new(p.sender.clone()) {
            Ok(entry) => entry,
            Err(err) => return render::rejected(err.to_string()),
        };

        let what = format!("the {} on this account", list_name(p.list));
        match self.list(&p.domain, p.list).remove(&entry).await {
            Ok(()) => render::confirmation(format!(
                "{} is off the account-wide {}, for every domain.",
                p.sender,
                list_name(p.list)
            )),
            Err(err) => failed(&err, Subject::new(&what, "mxroute_get_spam_settings")),
        }
    }
}

impl MxrouteServer {
    fn list<'a>(&'a self, domain: &'a str, list: params::SenderList) -> SenderListApi<'a> {
        let spam = self.client.spam(domain);
        match list {
            params::SenderList::Whitelist => spam.whitelist(),
            params::SenderList::Blacklist => spam.blacklist(),
        }
    }
}

fn list_name(list: params::SenderList) -> &'static str {
    match list {
        params::SenderList::Whitelist => "whitelist",
        params::SenderList::Blacklist => "blacklist",
    }
}

fn name(section: SpamSection) -> &'static str {
    match section {
        SpamSection::Threshold => "high_score",
        SpamSection::Whitelist => "whitelist",
        SpamSection::Blacklist => "blacklist",
    }
}
