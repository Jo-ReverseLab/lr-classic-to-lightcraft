//! Acceptance tests for the expanded CI configuration.
//!
//! The repository configuration is read as plain text: no YAML crate, no
//! network, no execution of the workflows themselves. Each test pins one part
//! of the contract that the developer has to fulfil. They are expected to be
//! red until that work is done.

use std::path::{Path, PathBuf};

const CI: &str = ".github/workflows/ci.yml";
const RELEASE: &str = ".github/workflows/release.yml";
const AUDIT: &str = ".github/workflows/audit.yml";
const PERFORMANCE: &str = ".github/workflows/performance.yml";
const LINK_CHECK: &str = "scripts/check-links.sh";
const PLAN: &str = "PLAN.md";
const WORKSPACE_MANIFEST: &str = "Cargo.toml";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Lines that belong to the block opened on `lines[header]`, i.e. the
/// following indented (or blank) lines until the next line at the same or a
/// lower indentation.
fn block<'a>(lines: &'a [&'a str], header: usize) -> Vec<&'a str> {
    let header_indent = indent(lines[header]);
    lines[header + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || indent(line) > header_indent)
        .copied()
        .collect()
}

fn mentions_main(line: &str) -> bool {
    let trimmed = line.trim().trim_start_matches("- ").trim();
    trimmed == "main" || (trimmed.starts_with("branches:") && trimmed.contains("main"))
}

#[test]
fn ci_workflow_is_present() {
    let text = read(CI);
    assert!(!text.trim().is_empty(), "{CI} must not be empty");
}

#[test]
fn ci_push_trigger_runs_on_main() {
    let text = read(CI);
    let lines: Vec<&str> = text.lines().collect();
    let push = lines
        .iter()
        .position(|line| line.trim() == "push:")
        .unwrap_or_else(|| panic!("{CI}: no `push:` trigger"));

    // The contract allows at most three lines between `push:` and the branch
    // restriction.
    let end = (push + 4).min(lines.len());
    let window = &lines[push + 1..end];
    assert!(
        window
            .iter()
            .any(|line| line.trim().starts_with("branches:")),
        "{CI}: `push:` must be limited to `branches:` within the next three lines"
    );
    assert!(
        window.iter().any(|line| mentions_main(line)),
        "{CI}: `push:` must run on `main` within the next three lines"
    );
}

#[test]
fn ci_pull_request_trigger_is_kept() {
    let text = read(CI);
    assert!(
        text.lines()
            .any(|line| line.trim().starts_with("pull_request:")),
        "{CI}: the `pull_request:` trigger must stay"
    );
}

#[test]
fn ci_cancels_superseded_runs() {
    let text = read(CI);
    let lines: Vec<&str> = text.lines().collect();
    let concurrency = lines
        .iter()
        .position(|line| line.trim().starts_with("concurrency:"))
        .unwrap_or_else(|| panic!("{CI}: no `concurrency:` key"));
    assert!(
        block(&lines, concurrency)
            .iter()
            .any(|line| line.trim() == "cancel-in-progress: true"),
        "{CI}: `concurrency:` must set `cancel-in-progress: true`"
    );
}

#[test]
fn ci_matrix_keeps_three_platforms() {
    let text = read(CI);
    assert!(
        text.lines().any(|line| line.trim().starts_with("matrix:")),
        "{CI}: no build matrix found"
    );
    for platform in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(
            text.contains(platform),
            "{CI}: the matrix lost `{platform}`"
        );
    }
}

#[test]
fn ci_keeps_format_clippy_and_test_steps() {
    let text = read(CI);
    for command in ["cargo fmt", "cargo clippy", "cargo test"] {
        assert!(
            text.lines()
                .any(|line| line.trim().starts_with("run:") && line.contains(command)),
            "{CI}: no step runs `{command}`"
        );
    }
}

/// `cargo fmt` is exempt: it does not build and rejects `--locked`. Every
/// command that resolves or builds dependencies must be locked, and clippy and
/// test must be covered explicitly.
#[test]
fn ci_cargo_commands_use_locked() {
    let text = read(CI);
    let commands = ["cargo clippy", "cargo test", "cargo check", "cargo build"];
    let mut clippy_locked = 0;
    let mut test_locked = 0;
    for line in text.lines() {
        if commands.iter().any(|command| line.contains(command)) {
            assert!(
                line.contains("--locked"),
                "{CI}: this command must use `--locked`: {line}"
            );
            if line.contains("cargo clippy") {
                clippy_locked += 1;
            }
            if line.contains("cargo test") {
                test_locked += 1;
            }
        }
    }
    assert!(
        clippy_locked > 0,
        "{CI}: expected a `cargo clippy` invocation with `--locked`"
    );
    assert!(
        test_locked > 0,
        "{CI}: expected a `cargo test` invocation with `--locked`"
    );
}

#[test]
fn ci_checks_msrv_1_88_0() {
    let text = read(CI);
    assert!(
        text.contains("1.88.0"),
        "{CI}: no MSRV job or step with `1.88.0`"
    );
    assert!(
        text.to_lowercase().contains("msrv"),
        "{CI}: the `1.88.0` entry must be a named MSRV job or step"
    );
}

#[test]
fn ci_actions_are_pinned_to_full_commit_shas() {
    let text = read(CI);
    let mut uses_lines = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(spec) = trimmed.strip_prefix("uses:") else {
            continue;
        };
        uses_lines += 1;
        // A trailing `# vX.Y.Z` comment is allowed after the pinned reference.
        let reference = spec.split_whitespace().next().unwrap_or("");
        let (action, sha) = reference.rsplit_once('@').unwrap_or_else(|| {
            panic!("{CI}: `{trimmed}` is not pinned; use `uses: <action>@<40 hex>`")
        });
        assert!(
            !action.is_empty(),
            "{CI}: `{trimmed}` has no action name before `@`"
        );
        assert!(
            sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
            "{CI}: `{trimmed}` must be pinned to a 40-character commit SHA, not `{sha}`"
        );
    }
    assert!(uses_lines > 0, "{CI}: no `uses:` steps found");
}

#[test]
fn ci_checks_personal_data_against_allowlist() {
    let text = read(CI);
    assert!(
        text.contains("personal data"),
        "{CI}: no personal-data check step (expected the words `personal data`)"
    );
    assert!(
        text.contains("/Users/example"),
        "{CI}: the personal-data check must use the `/Users/example` allowlist placeholder"
    );
}

#[test]
fn ci_runs_the_link_check() {
    let text = read(CI);
    assert!(
        text.contains("check-links.sh"),
        "{CI}: no step runs `check-links.sh`"
    );
}

#[test]
fn link_check_script_exists_and_is_not_empty() {
    let text = read(LINK_CHECK);
    assert!(!text.trim().is_empty(), "{LINK_CHECK} must not be empty");
}

#[test]
fn workspace_declares_msrv() {
    let text = read(WORKSPACE_MANIFEST);
    assert!(
        text.contains("rust-version = \"1.88.0\""),
        "{WORKSPACE_MANIFEST}: the workspace must declare `rust-version = \"1.88.0\"`"
    );
}

#[test]
fn release_workflow_guards_the_tag() {
    let text = read(RELEASE);
    for needle in ["refs/tags/v*", "test ! -e tasks", "CHANGELOG", "version"] {
        assert!(text.contains(needle), "{RELEASE}: missing `{needle}`");
    }
}

#[test]
fn audit_workflow_is_scheduled_and_fails_on_high_severity() {
    let text = read(AUDIT);
    assert!(
        text.contains("schedule:"),
        "{AUDIT}: no `schedule:` trigger"
    );
    assert!(text.contains("cron:"), "{AUDIT}: no `cron:` schedule");
    assert!(
        text.contains("cargo audit"),
        "{AUDIT}: does not run `cargo audit`"
    );
    assert!(
        text.contains("--json"),
        "{AUDIT}: the audit output must be machine-readable (`--json`)"
    );
    let lower = text.to_lowercase();
    assert!(
        lower.contains("high") || lower.contains("critical"),
        "{AUDIT}: `high` or `critical` findings must fail the job"
    );
}

#[test]
fn performance_workflow_is_scheduled_and_measures_in_release() {
    let text = read(PERFORMANCE);
    assert!(
        text.contains("schedule:"),
        "{PERFORMANCE}: no `schedule:` trigger"
    );
    assert!(text.contains("cron:"), "{PERFORMANCE}: no `cron:` schedule");
    assert!(
        text.contains("--ignored"),
        "{PERFORMANCE}: the measurement runs must be `--ignored` tests"
    );
    assert!(
        text.contains("--release"),
        "{PERFORMANCE}: the measurement runs must use `--release`"
    );
}

#[test]
fn plan_does_not_hardcode_the_repository_owner() {
    let text = read(PLAN);
    let marker = "gh api repos/";
    let mut checked = 0;
    for (index, _) in text.match_indices(marker) {
        checked += 1;
        let rest = &text[index + marker.len()..];
        assert!(
            rest.starts_with('<') || rest.starts_with("example"),
            "{PLAN}: `{marker}` must be followed by a placeholder such as \
             `<owner>/lr2lc`, not by a literal owner/repository pair"
        );
    }
    assert!(
        checked > 0,
        "{PLAN}: expected the documented `{marker}` command with a placeholder"
    );
}
