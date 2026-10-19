//! wind-core: the pure computation core (no I/O, no threads), shared by the
//! browser (WASM) build and the server build. Layout and rules:
//! docs/ARQUITETURA.md.
//!
//! Project rule from the Phase-0 benchmark: transcendental math goes through
//! the `libm` crate, never the platform CRT (slow on Windows-GNU).
#![warn(missing_docs)]

/// Version of the core, as recipes record it (ARQUITETURA C3).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod acc;
pub mod analysis;
pub mod append;
pub mod bootstrap;
pub mod calculated;
pub mod criteria;
pub mod derived;
pub mod displacement;
pub mod edits;
pub mod error;
pub mod expression;
pub mod extrapolation;
pub mod export;
pub mod filter;
pub mod flags;
pub mod hash;
pub mod history;
pub mod import;
pub mod mcp;
pub mod model;
pub mod ops;
pub mod primary;
pub mod region;
pub mod rules;
pub mod series;
pub mod shading;
pub mod time;
pub mod weibull;
