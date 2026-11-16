//! Progress reports and cancellation for front ends that show a run while it
//! goes (a GUI, or a host's activity list). The command line uses
//! [`NoProgress`].

/// What a run is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Computing the catalog checksum.
    Checksum,
    /// Reading the catalog.
    Reading,
    /// Checking that the original files exist.
    CheckingOriginals,
    /// Reading what the library holds now (second run).
    ReadingLibrary,
    /// Sending commands to LightCraft.
    Writing,
}

/// Receives progress and answers whether to stop.
///
/// A run calls [`Progress::phase`] when a phase starts, with the number of
/// units it has (0 when unknown), then [`Progress::advance`] with the units
/// done so far. [`Progress::cancelled`] is asked only where stopping is safe:
/// between photos, keywords and album steps while writing, never inside one
/// photo, so the mapping file always describes what LightCraft holds.
pub trait Progress {
    fn phase(&mut self, _phase: Phase, _total: usize) {}
    fn advance(&mut self, _done: usize) {}
    /// A step finished; called with its log entry as it happens.
    fn entry(&mut self, _entry: &model::Entry) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Reports nothing and never cancels.
pub struct NoProgress;

impl Progress for NoProgress {}
