//! Turning an API failure into something a caller can act on.
//!
//! Every one of these is a tool error rather than a protocol error: the request was valid
//! and reached the tool, so the caller should see what went wrong. A protocol error is
//! rendered opaquely by most clients and the message never arrives.

use mxroute::Error;
use rmcp::model::{CallToolResult, ContentBlock};

/// What the call was addressing, so the message can name it.
///
/// The templates below never prepend an article, because some subjects are bare nouns
/// (`domain example.com`) and others are not (`the catch-all for example.com`).
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// What was addressed, such as `mailbox sales@example.com`.
    pub what: &'a str,
    /// The tool that lists what does exist, when there is one.
    pub lister: Option<&'a str>,
}

impl<'a> Subject<'a> {
    pub fn new(what: &'a str, lister: &'a str) -> Self {
        Self {
            what,
            lister: Some(lister),
        }
    }

    /// For the tools that take no arguments, where nothing can be misnamed.
    pub fn account(what: &'a str) -> Self {
        Self { what, lister: None }
    }
}

/// Report a failed call, ending in what to do next.
pub fn failed(err: &Error, subject: Subject<'_>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message(err, subject))])
}

fn message(err: &Error, subject: Subject<'_>) -> String {
    let Subject { what, lister } = subject;
    let try_lister = |text: String| match lister {
        Some(lister) => format!("{text} {lister} lists what is there."),
        None => text,
    };

    if err.is_not_found() {
        return try_lister(format!("MXroute has no record of {what} on this account."));
    }

    if err.is_unauthorized() {
        return "MXroute rejected the credentials. Check MXROUTE_SERVER, MXROUTE_USERNAME and \
                MXROUTE_API_KEY against https://panel.mxroute.com/api-keys.php; every call \
                fails until they are right."
            .to_owned();
    }

    if err.is_forbidden() {
        return format!(
            "This API key may not reach {what}. A reseller endpoint needs a reseller account, \
             and a key issued for one server does not work on another."
        );
    }

    if err.is_validation() {
        return match err.api_error() {
            Some(api) => match &api.field {
                Some(field) => format!("MXroute rejected the value for `{field}`: {}", api.message),
                None => format!("MXroute rejected the request: {}", api.message),
            },
            None => format!("MXroute rejected the request: {err}"),
        };
    }

    if err.is_conflict() {
        return try_lister(format!("MXroute already has {what}."));
    }

    if err.is_rate_limited() {
        return "MXroute is throttling this account and the client has already paced itself \
                and retried. Reads are capped at 100 a minute and writes at 20; the same call \
                should succeed in about a minute."
            .to_owned();
    }

    if err.is_busy() {
        // The one case where retrying is the wrong reflex: the panel serializes these writes,
        // and a 503 says the result could not be confirmed rather than that nothing happened.
        return try_lister(format!(
            "MXroute was holding the panel lock, so the write to {what} may or may not have \
             applied. This client does not replay it. Read the current state back before \
             deciding whether to try again."
        ));
    }

    match err {
        Error::Decode { expected, .. } => format!(
            "MXroute answered with a body this client could not read as {expected}. The server \
             may be returning an error page instead of a response."
        ),
        Error::Transport(_) => format!("Could not reach the MXroute API: {err}"),
        // Already carries the method, path, status and a truncated body, and redacts the key.
        other => other.to_string(),
    }
}

// There are no unit tests here: `mxroute::Error` is `#[non_exhaustive]`, so its variants
// cannot be built from outside the library. The mapping is covered end to end in
// `tests/read_tools.rs`, where a mocked server answers with the statuses that produce them.
