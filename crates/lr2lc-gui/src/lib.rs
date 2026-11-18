//! Desktop window for lr2lc: choose a Lightroom Classic catalog, choose what
//! to transfer, run a dry run or the transfer into a LightCraft library and
//! read the run log.
//!
//! The window ([`app::App`]) draws into any `egui::Ui` and reaches its
//! surroundings only through [`host::Host`]; the conversion itself is
//! `lr2lc-core`, run in the same process. The standalone app
//! ([`host::StandaloneHost`]) talks to LightCraft over its control port.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

pub mod app;
pub mod checks;
pub mod existing;
pub mod history;
pub mod host;
pub mod icons;
pub mod log_view;
pub mod logdata;
pub mod run;
pub mod running;
pub mod settings;
pub mod theme;
pub mod transfer;
pub mod widgets;
