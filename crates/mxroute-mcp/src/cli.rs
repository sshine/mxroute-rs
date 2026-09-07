//! The command line, the environment, and turning both into a client.

use std::io::{self, IsTerminal as _};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use mxroute::{Client, Credentials};
use tracing_subscriber::EnvFilter;

use crate::render::OutputLimits;
use crate::server::Mode;

/// Serve the MXroute API to an MCP client over stdio.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// Mail server the account lives on, such as eagle.mxlogin.com.
    #[arg(long, env = "MXROUTE_SERVER")]
    server: Option<String>,

    /// DirectAdmin username, as shown on panel.mxroute.com.
    #[arg(long, env = "MXROUTE_USERNAME")]
    username: Option<String>,

    /// File holding the API key.
    ///
    /// The key is otherwise read from MXROUTE_API_KEY. There is deliberately no flag for
    /// the key itself: a command line is readable by every process on the host and is kept
    /// in shell history.
    #[arg(long, env = "MXROUTE_API_KEY_FILE")]
    api_key_file: Option<PathBuf>,

    /// Also serve the tools that change something.
    #[arg(long)]
    allow_writes: bool,

    /// Also serve the reseller tools, for an account that has them.
    #[arg(long)]
    reseller: bool,

    /// Base URL of the API, for pointing at something other than the live service.
    #[arg(long, env = "MXROUTE_BASE_URL")]
    base_url: Option<String>,

    /// Seconds to wait for one request before giving up.
    #[arg(long, default_value_t = 30)]
    timeout: u64,

    /// Rows a listing returns before it truncates.
    #[arg(long, default_value_t = 100)]
    max_items: usize,

    /// Print the tool schemas as JSON and exit.
    #[arg(long)]
    pub list_tools: bool,

    /// Check the credentials against the API and exit.
    #[arg(long)]
    pub check: bool,
}

impl Args {
    /// Which groups of tools to serve.
    ///
    /// The toggles are also readable from the environment, because a client that configures
    /// a server through an `env` block cannot always add arguments as well.
    pub fn mode(&self) -> Mode {
        Mode {
            writes: self.allow_writes || truthy("MXROUTE_MCP_ALLOW_WRITES"),
            reseller: self.reseller || truthy("MXROUTE_MCP_RESELLER"),
        }
    }

    /// How much one tool may return.
    pub fn limits(&self) -> OutputLimits {
        OutputLimits {
            max_items: self.max_items,
            ..OutputLimits::default()
        }
    }

    /// Build the one client the whole process shares.
    pub fn client(&self) -> Result<Client, StartupError> {
        let mut builder = Client::builder()
            .credentials(self.credentials()?)
            .user_agent(concat!("mxroute-mcp/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(self.timeout));

        if let Some(base_url) = present(self.base_url.clone()) {
            builder = builder.base_url(base_url);
        }

        builder
            .build()
            .map_err(|err| StartupError::Client(Box::new(err)))
    }

    fn credentials(&self) -> Result<Credentials, StartupError> {
        let server = present(self.server.clone()).ok_or(StartupError::Missing("MXROUTE_SERVER"))?;
        let username =
            present(self.username.clone()).ok_or(StartupError::Missing("MXROUTE_USERNAME"))?;
        Ok(Credentials::new(server, username, self.api_key()?))
    }

    fn api_key(&self) -> Result<String, StartupError> {
        match present(self.api_key_file.clone().map(|p| p.display().to_string())) {
            Some(path) => std::fs::read_to_string(&path)
                .map(|key| key.trim().to_owned())
                .map_err(|source| StartupError::ApiKeyFile {
                    path: PathBuf::from(path),
                    source,
                }),
            None => present(std::env::var("MXROUTE_API_KEY").ok())
                .ok_or(StartupError::Missing("MXROUTE_API_KEY")),
        }
    }
}

/// What went wrong before the first request.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(
        "{0} is not set. All three of MXROUTE_SERVER, MXROUTE_USERNAME and MXROUTE_API_KEY \
         are needed; create a key at https://panel.mxroute.com/api-keys.php and set them in \
         the `env` block of the MCP client's server configuration."
    )]
    Missing(&'static str),

    #[error("could not read the API key from {path}")]
    ApiKeyFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    // Boxed because `mxroute::Error` carries the request that failed and is far larger than
    // every other variant, which would make each Result in this module pay for it.
    #[error("could not build the API client")]
    Client(#[source] Box<mxroute::Error>),

    #[error("MXroute did not accept the credentials")]
    Rejected(#[source] Box<mxroute::Error>),

    #[error("could not write the tool schemas")]
    Schemas(#[from] serde_json::Error),
}

/// Send the log to stderr, because stdout carries the protocol.
///
/// A single line of anything else on stdout ends the session: the client reads the stream as
/// framed JSON-RPC and treats a parse failure as the server crashing. `MXROUTE_MCP_LOG` wins
/// over `RUST_LOG` so that a filter inherited from the client's own environment does not turn
/// the log pane into a firehose.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_env("MXROUTE_MCP_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("mxroute_mcp=info,mxroute=warn"));

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .with_env_filter(filter)
        .init();
}

/// Treat an empty value as absent.
///
/// A Claude Code plugin substitutes `${user_config.server}` into the server's environment
/// whether or not the user filled the field in, so an unanswered prompt arrives as an empty
/// string that would otherwise mask a value exported in the shell.
fn present(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// Anything but the empty string, `0`, `false` and `no` turns a toggle on.
fn truthy(var: &str) -> bool {
    std::env::var(var).is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_value_counts_as_absent() {
        assert_eq!(present(Some("  ".to_owned())), None);
        assert_eq!(present(Some(String::new())), None);
        assert_eq!(present(None), None);
        assert_eq!(present(Some("x".to_owned())), Some("x".to_owned()));
    }

    #[test]
    fn a_toggle_reads_the_usual_spellings_of_off() {
        for off in ["", "0", "false", "FALSE", " no "] {
            unsafe { std::env::set_var("MXROUTE_MCP_TEST_TOGGLE", off) };
            assert!(!truthy("MXROUTE_MCP_TEST_TOGGLE"), "{off:?} should be off");
        }
        for on in ["1", "true", "yes", "anything"] {
            unsafe { std::env::set_var("MXROUTE_MCP_TEST_TOGGLE", on) };
            assert!(truthy("MXROUTE_MCP_TEST_TOGGLE"), "{on:?} should be on");
        }
        unsafe { std::env::remove_var("MXROUTE_MCP_TEST_TOGGLE") };
        assert!(!truthy("MXROUTE_MCP_TEST_TOGGLE"));
    }
}
