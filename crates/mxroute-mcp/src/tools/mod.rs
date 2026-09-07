//! The tools, grouped the way the API is, and split by whether they change anything.
//!
//! Each group contributes a router per half, which [`MxrouteServer::router`] composes
//! according to the mode. Reads and writes are separate tools throughout: a client decides
//! whether to ask for confirmation from a tool's annotations, and a tool that both reads and
//! writes leaves it nothing to decide on.
//!
//! [`MxrouteServer::router`]: crate::server::MxrouteServer::router

pub mod catch_all;
pub mod domains;
pub mod forwarders;
pub mod mailboxes;
pub mod quota;
pub mod spam;
