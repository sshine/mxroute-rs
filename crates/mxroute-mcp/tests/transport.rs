//! What the real binary puts on its two output streams.
#![allow(clippy::expect_used)]

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::Value;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
const LIST_TOOLS: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

/// Nothing but framed JSON may reach stdout, whatever the log filter says.
///
/// This is the one failure that breaks everything silently: a client reads stdout as
/// JSON-RPC frames, so a stray log line is a parse failure, which it reports as the server
/// having crashed rather than as a logging mistake. Trace level is the worst case, and the
/// handshake alone reaches it without needing the API.
#[test]
fn the_log_never_lands_on_the_protocol_stream() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mxroute-mcp"))
        .env("MXROUTE_SERVER", "eagle.example.com")
        .env("MXROUTE_USERNAME", "johndoe")
        .env("MXROUTE_API_KEY", "Mx8d989005f0cded8371b7d7271c50K1")
        .env("MXROUTE_MCP_LOG", "trace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary should start");

    let mut stdin = child.stdin.take().expect("stdin was piped");
    writeln!(stdin, "{INITIALIZE}").expect("the server should accept the handshake");
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .expect("the server should accept the notification");
    writeln!(stdin, "{LIST_TOOLS}").expect("the server should accept the listing request");
    drop(stdin);

    let output = child.wait_with_output().expect("the server should exit");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "expected two responses, got:\n{stdout}");
    for line in &lines {
        serde_json::from_str::<Value>(line).unwrap_or_else(|err| {
            panic!("stdout carried something that is not a frame: {err}\n{line}")
        });
    }

    // The log did happen; it just went to the other stream.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("TRACE") || stderr.contains("DEBUG"),
        "{stderr}"
    );
}

/// Refusing to start says which variable is missing, and says it on stderr.
#[test]
fn a_refused_start_explains_itself_without_touching_stdout() {
    let output = Command::new(env!("CARGO_BIN_EXE_mxroute-mcp"))
        .env_remove("MXROUTE_SERVER")
        .env_remove("MXROUTE_USERNAME")
        .env_remove("MXROUTE_API_KEY")
        .env_remove("MXROUTE_API_KEY_FILE")
        .output()
        .expect("the binary should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MXROUTE_SERVER"), "{stderr}");
    assert!(stderr.contains("panel.mxroute.com"), "{stderr}");
}

/// An unanswered plugin prompt arrives as an empty string, which must not mask the shell.
#[test]
fn an_empty_credential_is_treated_as_missing() {
    let output = Command::new(env!("CARGO_BIN_EXE_mxroute-mcp"))
        .env("MXROUTE_SERVER", "")
        .env("MXROUTE_USERNAME", "johndoe")
        .env("MXROUTE_API_KEY", "Mx8d989005f0cded8371b7d7271c50K1")
        .output()
        .expect("the binary should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("MXROUTE_SERVER"),
        "an empty value should read as absent"
    );
}

/// The surface can be inspected before any credentials exist.
#[test]
fn the_tool_schemas_print_without_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_mxroute-mcp"))
        .arg("--list-tools")
        .env_remove("MXROUTE_SERVER")
        .env_remove("MXROUTE_USERNAME")
        .env_remove("MXROUTE_API_KEY")
        .output()
        .expect("the binary should run");

    assert!(output.status.success());
    let tools: Vec<Value> =
        serde_json::from_slice(&output.stdout).expect("--list-tools should print JSON");
    assert!(!tools.is_empty());
}
