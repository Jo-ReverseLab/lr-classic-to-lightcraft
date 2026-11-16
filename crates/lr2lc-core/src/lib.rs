//! Conversion core of lr2lc, shared by the command line and other front
//! ends: reads a Lightroom Classic catalog, plans the LightCraft commands and
//! writes them through the control port. Front ends drive a run with
//! [`prepare`] and [`finish`]; nothing here prints.

pub mod dry_run;
mod job;
pub mod labels;
pub mod output;
pub mod report_file;
pub mod writer;

pub use job::{ErrorKind, Finished, Job, Prepared, RunError, finish, prepare};
