//! An async client for the [MXroute] email hosting API.
//!
//! [MXroute]: https://mxroute.com
//!
//! The API is a REST facade over DirectAdmin, so a client authenticates against one mail
//! server at a time and every request carries three headers rather than a single token.
//!
//! This crate is under construction; the endpoint modules land in subsequent commits.

#![warn(missing_docs)]

pub mod api;
mod client;
mod error;
mod ratelimit;

pub use client::{
    Client, ClientBuilder, Credentials, DEFAULT_BASE_URL, DEFAULT_USER_AGENT, MAX_RETRY_AFTER,
    Secret,
};
pub use error::{ApiError, Error, ErrorCode, InvalidValue, Result};
pub use ratelimit::{Rate, RateLimits, Scope};
