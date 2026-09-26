//! rbirds: cbirds 1.4.0 (commit cc446fc3cb80733371c62676533adcac2fc10002),
//! a flock of birds in the terminal, translated into Rust.
//!
//! The library exists so integration tests can drive each translated module
//! and compare it with the pinned C reference; the `rbirds` binary is a thin
//! entry point over [`app::main`]. See docs/DESIGN.md for the architecture and
//! docs/COMPATIBILITY.md for the behavior it must preserve.

#![deny(unsafe_code)]

pub mod app;
pub mod bench;
pub mod cfmt;
pub mod config;
pub mod font;
pub mod fp;
pub mod image;
pub mod input;
pub mod live;
pub mod options;
pub mod palette;
#[cfg_attr(windows, path = "platform/windows.rs")]
pub mod platform;
pub mod record;
pub mod render;
pub mod rng;
pub mod simulation;
pub mod spatial_grid;
pub mod sprites;
pub mod stdio;
pub mod terminal;
