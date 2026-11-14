//! RustChat application layer: use cases and the ports they depend on.

pub mod auth;
pub mod error;
pub mod ports;

#[cfg(test)]
mod testing;
