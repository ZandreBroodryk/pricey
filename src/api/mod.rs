//! Server functions -- the RPC surface between the browser and the database.
//!
//! Every function that touches data starts by resolving the session, and every query is
//! scoped to the owning user in SQL. Ids arriving from the client are never trusted.

pub mod auth;
pub mod history;
pub mod items;
pub mod refresh;
pub mod sources;
