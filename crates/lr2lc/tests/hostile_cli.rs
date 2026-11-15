//! Hostile-input tests at the CLI level: each damaged catalog is run as a dry
//! run (`lr2lc <catalog>`) and must exit cleanly — success (0, skips in the
//! report), a read error (1), or a usage error (2) — never a crash signal.
//!
//! Companion to `crates/lr-reader/tests/hostile.rs`, which checks the reader
//! pipeline directly.

use lrcat_fixtures::hostile::{HostileCase, write_hostile_catalog};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
}

#[test]
fn no_hostile_case_crashes_the_cli() {
    for &case in HostileCase::all() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = write_hostile_catalog(dir.path(), case).unwrap();
        let out = bin().arg(&catalog).output().unwrap();
        // The process must exit with a code, not be killed by a signal
        // (SIGSEGV etc.). A clean exit is 0 (skips reported), 1 (read error)
        // or 2 (usage error); all are acceptable for a damaged catalog.
        let code = out.status.code();
        assert!(
            matches!(code, Some(0) | Some(1) | Some(2)),
            "{}: CLI crashed or exited with unexpected code {:?}; stderr: {}",
            case.slug(),
            code,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
