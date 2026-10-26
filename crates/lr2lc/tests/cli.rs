//! Acceptance tests for the CLI skeleton (task P3-1).
//!
//! Dry run is the default (principle 3): without `--apply` nothing is written.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lr2lc"))
}

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lr2lc-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn help_exits_zero_and_mentions_apply() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("--apply"));
}

#[test]
fn version_exits_zero_and_prints_version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_flag_fails() {
    let out = bin().arg("--no-such-flag").output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn no_apply_no_args_writes_nothing() {
    let dir = unique_dir("noapply");
    let out = bin().current_dir(&dir).output().unwrap();
    let code = out.status.code();
    assert!(matches!(code, Some(0) | Some(2)), "unexpected exit: {code:?}");
    let entries = std::fs::read_dir(&dir).unwrap().count();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(entries, 0, "dry run must not write anything");
}
