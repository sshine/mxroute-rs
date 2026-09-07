//! What each mode serves, and what it refuses to serve.

mod common;

use common::serve;
use mxroute_mcp::server::Mode;
use serde_json::json;

const READS: [&str; 8] = [
    "mxroute_get_dns_records",
    "mxroute_get_domain",
    "mxroute_get_quota",
    "mxroute_get_spam_settings",
    "mxroute_get_verification_key",
    "mxroute_list_domains",
    "mxroute_list_forwarders",
    "mxroute_list_mailboxes",
];

const WRITES: [&str; 14] = [
    "mxroute_add_spam_sender",
    "mxroute_create_domain",
    "mxroute_create_forwarder",
    "mxroute_create_mailbox",
    "mxroute_create_pointer",
    "mxroute_delete_domain",
    "mxroute_delete_forwarder",
    "mxroute_delete_mailbox",
    "mxroute_delete_pointer",
    "mxroute_remove_spam_sender",
    "mxroute_set_catch_all",
    "mxroute_set_mail_hosting",
    "mxroute_set_spam_score",
    "mxroute_update_mailbox",
];

const RESELLER_READS: [&str; 2] = [
    "mxroute_list_reseller_packages",
    "mxroute_list_reseller_users",
];

const RESELLER_WRITES: [&str; 8] = [
    "mxroute_create_reseller_package",
    "mxroute_create_reseller_user",
    "mxroute_delete_reseller_package",
    "mxroute_delete_reseller_user",
    "mxroute_set_reseller_user_package",
    "mxroute_set_reseller_user_suspended",
    "mxroute_update_reseller_package",
    "mxroute_update_reseller_user",
];

fn sorted<'a>(groups: &[&[&'a str]]) -> Vec<&'a str> {
    let mut names: Vec<&str> = groups.iter().flat_map(|g| g.iter().copied()).collect();
    names.sort_unstable();
    names
}

async fn served(mode: Mode) -> Vec<String> {
    let h = serve(mode).await;
    let mut names: Vec<String> = h.tools().await.into_iter().map(|t| t.name.into()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn the_default_mode_serves_only_the_tools_that_read() {
    assert_eq!(served(Mode::default()).await, READS);
}

#[tokio::test]
async fn allowing_writes_adds_exactly_the_tools_that_write() {
    let mode = Mode {
        writes: true,
        reseller: false,
    };
    assert_eq!(served(mode).await, sorted(&[&READS, &WRITES]));
}

#[tokio::test]
async fn the_reseller_flag_alone_adds_only_the_reseller_tools_that_read() {
    // The two toggles are independent: asking for reseller without asking for writes must
    // not smuggle the reseller writes in with it.
    let mode = Mode {
        writes: false,
        reseller: true,
    };
    assert_eq!(served(mode).await, sorted(&[&READS, &RESELLER_READS]));
}

#[tokio::test]
async fn both_flags_serve_the_whole_surface() {
    let mode = Mode {
        writes: true,
        reseller: true,
    };
    let expected = sorted(&[&READS, &WRITES, &RESELLER_READS, &RESELLER_WRITES]);
    assert_eq!(expected.len(), 32);
    assert_eq!(served(mode).await, expected);
}

#[tokio::test]
async fn a_write_tool_cannot_be_reached_by_name_when_it_was_not_served() {
    // Composed rather than filtered, so the name does not resolve at all. A tool that is
    // merely hidden is still callable by anything that knows what to ask for.
    let h = serve(Mode::default()).await;
    let result = h
        .mcp
        .call_tool(rmcp::model::CallToolRequestParams::new(
            "mxroute_delete_domain",
        ))
        .await;
    assert!(result.is_err(), "an unserved tool should not resolve");
}

#[tokio::test]
async fn deleting_a_domain_needs_the_name_twice_before_any_request_is_made() {
    // No mock is mounted: if the guard let this through, the call would fail as an
    // unmatched request instead, and the assertion below would not distinguish them.
    let h = serve(Mode {
        writes: true,
        reseller: false,
    })
    .await;

    let text = h
        .error(
            "mxroute_delete_domain",
            json!({"domain": "a.example", "confirm_domain": "b.example"}),
        )
        .await;
    assert!(text.contains("confirm_domain"), "{text}");
    assert_eq!(
        h.http.received_requests().await.unwrap_or_default().len(),
        0
    );
}
