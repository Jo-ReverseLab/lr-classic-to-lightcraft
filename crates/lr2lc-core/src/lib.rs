//! Conversion core of lr2lc, shared by the command line and other front
//! ends: reads a Lightroom Classic catalog, plans the LightCraft commands and
//! writes them through the control port. Front ends drive a run with
//! [`prepare`] and [`finish`]; nothing here prints.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

pub mod dry_run;
mod guidance;
mod job;
pub mod labels;
pub mod log;
pub mod output;
pub mod report_file;
pub mod writer;

pub use job::{ErrorKind, Finished, Job, Prepared, RunError, finish, finish_with, prepare};
/// Re-exported so a front end needs no direct dependency on lc-writer to
/// write through its own [`CommandSink`].
pub use lc_writer::control::{CommandSink, HostSink, Request};
pub use lc_writer::plan::Areas;
pub use lc_writer::progress::{NoProgress, Phase, Progress};
