//! rbirds: cbirds 1.4.0 (commit cc446fc3cb80733371c62676533adcac2fc10002),
//! a flock of birds in the terminal, translated into Rust.
//!
//! The library exists so integration tests can drive each translated module
//! and compare it with the pinned C reference; the `rbirds` binary is a thin
//! entry point over [`app`]. See docs/DESIGN.md for the architecture and
//! docs/COMPATIBILITY.md for the behavior it must preserve.

#![deny(unsafe_code)]

pub mod font;
pub mod fp;
pub mod image;
pub mod options;
pub mod platform;
pub mod render;
pub mod rng;
pub mod spatial_grid;
