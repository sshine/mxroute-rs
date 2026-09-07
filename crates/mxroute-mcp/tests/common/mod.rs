//! Shared harness: a mocked API underneath, a real MCP client and server on top.
//!
//! The lower half duplicates `crates/mxroute/tests/common/mod.rs`. A `tests/common` module
//! belongs to one crate and cannot be reached from another, and the alternatives are a third
//! crate published for nobody or a test-support feature on a public API that has been kept
//! deliberately small. Copying forty lines is the cheaper of the three.
#![allow(clippy::expect_used, dead_code)]

use mxroute::{Client, Credentials, RateLimits};
use mxroute_mcp::render::OutputLimits;
use mxroute_mcp::server::{Mode, MxrouteServer};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt as _};
use serde_json::{Value, json};
use wiremock::MockServer;
use wiremock::matchers::header;

/// The credentials every mocked test authenticates with.
pub const SERVER: &str = "eagle.mxlogin.com";
pub const USERNAME: &str = "johndoe";
pub const API_KEY: &str = "Mx8d989005f0cded8371b7d7271c50K1";

/// A mocked API, and an MCP client talking to a server that talks to it.
pub struct Harness {
    /// Mount expectations here.
    pub http: MockServer,
    /// Call tools through here.
    pub mcp: RunningService<RoleClient, ()>,
}

impl Harness {
    /// Call a tool and return whatever it answered, error results included.
    pub async fn call(&self, name: &'static str, arguments: Value) -> CallToolResult {
        let mut request = CallToolRequestParams::new(name);
        if let Value::Object(map) = arguments {
            request = request.with_arguments(map);
        }
        self.mcp
            .call_tool(request)
            .await
            .expect("the call itself should reach the tool")
    }

    /// The structured half of a successful result.
    pub async fn read(&self, name: &'static str, arguments: Value) -> Value {
        let result = self.call(name, arguments).await;
        assert_ne!(result.is_error, Some(true), "{name} failed: {result:?}");
        result
            .structured_content
            .expect("a read tool should answer with structured content")
    }

    /// The text of a failed result.
    pub async fn error(&self, name: &'static str, arguments: Value) -> String {
        let result = self.call(name, arguments).await;
        assert_eq!(result.is_error, Some(true), "{name} unexpectedly succeeded");
        text(&result)
    }

    /// Every tool this server serves.
    pub async fn tools(&self) -> Vec<Tool> {
        self.mcp
            .list_all_tools()
            .await
            .expect("the server should list its tools")
    }
}

/// Start a mocked API and a server serving `mode` against it.
pub async fn serve(mode: Mode) -> Harness {
    serve_with(mode, OutputLimits::default()).await
}

/// As [`serve`], with the output budget under test.
pub async fn serve_with(mode: Mode, limits: OutputLimits) -> Harness {
    let http = MockServer::start().await;
    let client = client_for(&http);

    // A pipe rather than a subprocess: the same transport the binary uses over stdio, so the
    // framing is exercised, without paying to spawn and shut down a process per test.
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(service) = MxrouteServer::new(client, mode, limits)
            .serve(server_io)
            .await
        {
            let _ = service.waiting().await;
        }
    });

    let mcp = ().serve(client_io).await.expect("the client should complete the handshake");

    Harness { http, mcp }
}

/// A client pointed at `server`, with no local rate limits and no retries so nothing sleeps.
pub fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .credentials(Credentials::new(SERVER, USERNAME, API_KEY))
        .base_url(server.uri())
        .rate_limits(RateLimits::unlimited())
        .max_retries(0)
        .build()
        .expect("valid configuration")
}

/// Matchers asserting all three credential headers are present and correct.
///
/// Applied to every mock, so a request that forgot one does not match and the test fails with
/// an unmatched request rather than a passing assertion.
pub fn auth() -> [wiremock::matchers::HeaderExactMatcher; 3] {
    [
        header("x-server", SERVER),
        header("x-username", USERNAME),
        header("x-api-key", API_KEY),
    ]
}

/// The concatenated text of a result, which is where a tool error's message lives.
pub fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A success envelope around `data`, the shape almost every endpoint answers with.
pub fn ok(data: Value) -> Value {
    json!({ "success": true, "data": data })
}

/// An error document.
pub fn err(code: &str, message: &str) -> Value {
    json!({ "success": false, "error": { "code": code, "message": message } })
}

/// An error document naming the field that was rejected.
pub fn err_field(code: &str, message: &str, field: &str) -> Value {
    json!({
        "success": false,
        "error": { "code": code, "message": message, "field": field },
    })
}

/// A mailbox as the API reports one.
pub fn email_account_json(username: &str, quota: u32) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@example.com"),
        "quota": quota,
        "usage": 256.5,
        "limit": 9600,
        "sent": 42,
        "suspended": false,
    })
}

/// A domain as the API reports one.
pub fn domain_json(domain: &str) -> Value {
    json!({
        "domain": domain,
        "mail_hosting": true,
        "ssl_enabled": true,
        "pointers": [],
    })
}
