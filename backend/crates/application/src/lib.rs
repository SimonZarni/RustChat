//! RustChat application layer: use cases and the ports they depend on.

pub mod auth;
pub mod communities;
pub mod conversations;
pub mod error;
pub mod messages;
pub mod ports;
pub mod ratelimit;

#[cfg(test)]
mod testing;
