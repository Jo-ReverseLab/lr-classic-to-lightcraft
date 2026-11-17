//! Writes the neutral model into a LightCraft library.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

mod albums;
pub mod apply;
pub mod control;
pub mod entries;
pub mod live;
pub mod plan;
pub mod progress;
pub mod rollback;
pub mod smart;
pub mod state;
