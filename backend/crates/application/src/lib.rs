//! RustChat application layer: use cases and the ports they depend on.

pub mod attachments;
pub mod auth;
pub mod communities;
pub mod conversations;
pub mod error;
pub mod messages;
pub mod notifications;
pub mod ports;
pub mod ratelimit;
pub mod social;

#[cfg(test)]
mod testing;
