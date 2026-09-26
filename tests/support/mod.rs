//! Shared helpers for the integration tests. Each test binary includes this
//! with `mod support;` and uses what it needs.

#![allow(dead_code)]

pub mod oracle;
#[cfg(unix)]
pub mod pty;
pub mod sim;
pub mod suite;
