//! What the tools accept.
//!
//! The schemas are as tight as the API's own rules allow, because a call rejected here
//! costs nothing while one rejected by the server costs a request and a rate-limit slot.

use schemars::JsonSchema;
use serde::Deserialize;

/// A domain on the account.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Domain {
    /// Domain name, such as "example.com". No scheme, no trailing dot.
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,
}

/// The mailboxes of one domain, or one of them.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListMailboxes {
    /// Domain the mailboxes belong to, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Local part of one mailbox, without the "@domain" suffix. Given, only that mailbox
    /// is returned, and an unknown one is reported rather than silently empty.
    #[serde(default)]
    #[schemars(length(min = 1, max = 64))]
    pub username: Option<String>,
}

/// Which parts of a domain's spam configuration to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpamSection {
    /// The score at or above which a message is treated as spam.
    Threshold,
    /// Senders whose mail is never treated as spam.
    Whitelist,
    /// Senders whose mail is always treated as spam.
    Blacklist,
}

impl SpamSection {
    pub const ALL: [Self; 3] = [Self::Threshold, Self::Whitelist, Self::Blacklist];
}

/// A domain's spam configuration.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SpamSettings {
    /// Domain to reach the settings through, such as "example.com".
    #[schemars(length(min = 1, max = 253), regex(pattern = r"^[A-Za-z0-9.-]+$"))]
    pub domain: String,

    /// Which sections to fetch, each costing one request. Omitted, all three are fetched.
    #[serde(default)]
    pub include: Option<Vec<SpamSection>>,
}

impl SpamSettings {
    /// The sections to fetch, with the empty selection meaning all of them rather than none.
    pub fn sections(&self) -> Vec<SpamSection> {
        match &self.include {
            Some(sections) if !sections.is_empty() => sections.clone(),
            _ => SpamSection::ALL.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_for_no_spam_section_asks_for_all_of_them() {
        let all = SpamSection::ALL.to_vec();
        for include in [None, Some(Vec::new())] {
            let params = SpamSettings {
                domain: "example.com".to_owned(),
                include,
            };
            assert_eq!(params.sections(), all);
        }
    }

    #[test]
    fn a_named_spam_section_is_the_only_one_fetched() {
        let params = SpamSettings {
            domain: "example.com".to_owned(),
            include: Some(vec![SpamSection::Blacklist]),
        };
        assert_eq!(params.sections(), vec![SpamSection::Blacklist]);
    }
}
