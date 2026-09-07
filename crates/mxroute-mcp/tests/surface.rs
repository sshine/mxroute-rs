//! The rules the tool surface has to keep, as assertions rather than as a checklist.
//!
//! These are the MCP directory's review criteria plus the ones this server sets for itself.
//! Every one of them is the kind of thing that is easy to forget on the tool added next
//! month and invisible until a client behaves oddly in front of a user.

mod common;

use common::serve;
use mxroute_mcp::server::{Mode, MxrouteServer};
use rmcp::model::Tool;

fn everything() -> Vec<Tool> {
    MxrouteServer::router(Mode {
        writes: true,
        reseller: true,
    })
    .list_all()
}

#[test]
fn every_tool_states_its_annotations_rather_than_leaving_them_to_be_assumed() {
    // rmcp leaves each hint as None unless it is given, and a host then assumes the worst:
    // an unannotated read is treated as destructive and prompts the user for nothing.
    for tool in everything() {
        let name = &tool.name;
        let annotations = tool
            .annotations
            .unwrap_or_else(|| panic!("{name} has no annotations at all"));

        assert!(annotations.title.is_some(), "{name} has no title");
        assert!(
            annotations.read_only_hint.is_some(),
            "{name} does not say whether it reads only"
        );
        assert!(
            annotations.destructive_hint.is_some(),
            "{name} does not say whether it is destructive"
        );
        assert!(
            annotations.open_world_hint.is_some(),
            "{name} does not say whether it reaches an open world"
        );
    }
}

#[test]
fn nothing_claims_to_be_both_read_only_and_destructive() {
    for tool in everything() {
        let Some(annotations) = tool.annotations else {
            continue;
        };
        if annotations.read_only_hint == Some(true) {
            assert_eq!(
                annotations.destructive_hint,
                Some(false),
                "{} is read-only but claims to be destructive",
                tool.name
            );
        }
    }
}

#[test]
fn every_name_is_prefixed_and_within_the_length_a_client_will_accept() {
    for tool in everything() {
        let name = tool.name.as_ref();
        assert!(name.len() <= 64, "{name} is over the 64-character limit");
        assert!(
            name.starts_with("mxroute_"),
            "{name} is not prefixed, so it collides in a client that does not namespace"
        );
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{name} is not lowercase_with_underscores"
        );
    }
}

#[test]
fn every_tool_and_every_argument_is_described() {
    for tool in everything() {
        let name = &tool.name;
        let description = tool
            .description
            .as_deref()
            .unwrap_or_else(|| panic!("{name} has no description"));
        assert!(
            !description.trim().is_empty(),
            "{name} has an empty description"
        );

        let Some(properties) = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (argument, schema) in properties {
            let described = schema
                .get("description")
                .and_then(|d| d.as_str())
                .is_some_and(|d| !d.trim().is_empty());
            assert!(described, "{name}.{argument} has no description");
        }
    }
}

#[test]
fn no_description_tries_to_tell_the_model_how_to_behave() {
    // A description is a manpage entry, not an instruction. Directives here are read as
    // prompt injection at review, and they are what the server's own instructions are for.
    const DIRECTIVES: [&str; 7] = [
        "always ",
        "never ",
        "you should",
        "you must",
        "make sure to",
        "be sure to",
        "do not use",
    ];

    for tool in everything() {
        let lowered = tool
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        for directive in DIRECTIVES {
            assert!(
                !lowered.contains(directive),
                "{} instructs the model: {directive:?}",
                tool.name
            );
        }
    }
}

#[test]
fn the_whole_surface_stays_inside_what_a_context_window_should_carry() {
    // Past roughly thirty, one tool per operation stops paying for itself and the surface
    // wants a search-and-execute pair instead. Crossing it should be a decision, not a drift.
    let total = everything().len();
    assert!(total <= 32, "the surface has grown to {total} tools");
}

#[tokio::test]
async fn the_served_names_are_exactly_these() {
    // Pinned so that adding or renaming a tool shows up as a deliberate diff here, next to
    // the rules above, rather than only in whatever a client happens to display.
    let h = serve(Mode {
        writes: true,
        reseller: true,
    })
    .await;

    let mut names: Vec<String> = h.tools().await.into_iter().map(|t| t.name.into()).collect();
    names.sort();

    assert_eq!(
        names,
        [
            "mxroute_add_spam_sender",
            "mxroute_create_domain",
            "mxroute_create_forwarder",
            "mxroute_create_mailbox",
            "mxroute_create_pointer",
            "mxroute_create_reseller_package",
            "mxroute_create_reseller_user",
            "mxroute_delete_domain",
            "mxroute_delete_forwarder",
            "mxroute_delete_mailbox",
            "mxroute_delete_pointer",
            "mxroute_delete_reseller_package",
            "mxroute_delete_reseller_user",
            "mxroute_get_dns_records",
            "mxroute_get_domain",
            "mxroute_get_quota",
            "mxroute_get_spam_settings",
            "mxroute_get_verification_key",
            "mxroute_list_domains",
            "mxroute_list_forwarders",
            "mxroute_list_mailboxes",
            "mxroute_list_reseller_packages",
            "mxroute_list_reseller_users",
            "mxroute_remove_spam_sender",
            "mxroute_set_catch_all",
            "mxroute_set_mail_hosting",
            "mxroute_set_reseller_user_package",
            "mxroute_set_reseller_user_suspended",
            "mxroute_set_spam_score",
            "mxroute_update_mailbox",
            "mxroute_update_reseller_package",
            "mxroute_update_reseller_user",
        ]
    );
}
