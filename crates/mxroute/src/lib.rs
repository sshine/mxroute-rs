//! An async client for the [MXroute] email hosting API.
//!
//! [MXroute]: https://mxroute.com
//!
//! The API is a REST facade over DirectAdmin, so a client authenticates against one mail
//! server at a time and every request carries three headers rather than a single token.
//!
//! This crate is under construction; the endpoint modules land in subsequent commits.

#![warn(missing_docs)]
