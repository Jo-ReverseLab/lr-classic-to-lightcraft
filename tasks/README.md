# Working rules for the external agent

This directory is the hand-off point between the maintainers' review process and the
external agent (GLM). The agent reads its tasks from `tasks/TASKS.md`, works on them
in its own branch, pushes that branch, and hands over a short report. The reviewer
then checks, corrects and merges. The agent never merges.

This directory is a working channel, not part of the product. It is removed before the
first public release.

## 0. The one hard rule: where you may push

**You may push only to a branch that you created yourself, in your own git worktree,
and that is named `glm/<task-id>` (for example `glm/t01-auto-label-set`).
Pushing anywhere else is forbidden. No exception, not even "just a small fix".**

Concretely:

- Never push to `main`, to any `agent/*` branch, to `fix/*` branches, or to any branch you
  did not create. Never create or push tags.
- Always push with an explicit refspec to your own branch:
  `git push -u origin glm/<task-id>`
  Never use `git push` without arguments, `--all`, `--mirror`, `--tags`, `--force`,
  `--force-with-lease`, or `--delete`. If a push is rejected, do not work around it:
  stop, write the reason into your report, and continue with another task.
- Never rewrite published history. Fix mistakes with a new commit.
- Do not change anything under `.github/` (CI workflows, templates). Your token has no
  right to, and a change there is a reason to reject the whole branch.
- Never change repository settings, branch rules, secrets, or collaborators.

Create your own worktree for every task, branching from the current `origin/main`:

```sh
git fetch origin
git worktree add ../lr2lc-glm-<task-id> -b glm/<task-id> origin/main
```

Work only in that worktree. One task = one branch = one worktree. Tasks that depend on
another task say so; in that case branch from the branch named there, not from `main`.

## 1. Privacy and content rules (same as for every contributor)

- Everything in the repository is English: code, comments, docs, commit messages.
- No real Lightroom catalogs, no extracts of one, no file names, keywords, people, GPS or
  paths from a real catalog. Tests use catalogs produced by the fixture generator in
  `tests/fixtures/generator`. You have no access to any real catalog, and nothing in a task
  asks you to look for one.
- No personal paths, user names, host names, e-mail addresses or tokens anywhere in code,
  tests, docs or commit messages. Use neutral placeholders (`example`, `C:\data\...`).
- Reports (`report.json` and its readable summary) never contain field values from a
  catalog, only field names and ids.
- No new Rust dependencies. Allowed crates: `serde`, `serde_json`, `clap`, `thiserror`,
  `anyhow`, `rusqlite`, `flate2`, `chrono`, `sha2`, `walkdir`, and `tempfile` (tests only).
  If you believe you need another crate, do not add it: write the question into your report.
- No `unsafe`. Rust stable. `cargo fmt` clean. `cargo clippy --all-targets -- -D warnings`
  clean. Libraries use `thiserror`, the binary uses `anyhow`.
- Follow `CLAUDE.md` in the repository root. Its principles decide every design question
  (read Lightroom only, never touch originals, dry run by default, nothing lost silently,
  idempotent, runs everywhere, three separate parts, official LightCraft interfaces,
  private data stays private, small and readable).
- Do not copy code from lrcat-extractor (MPL-2.0). Reading it is fine, writing is your own.

## 2. How to work on a task

1. Read the task in `tasks/TASKS.md` completely, then the files it names.
2. Tests first: write or enable the acceptance tests, commit them separately
   ("Add tests for ..."), then the implementation in later commits.
3. Small commits, imperative English messages ("Add folder reader"). Each commit must
   stand alone and build. Do not use `--no-verify`. If a hook rejects a commit, fix the cause.
4. Verification: if your workspace has no C toolchain and cannot run `cargo test`, say so
   in the report and keep every commit small, so that the CI run on your branch pins any
   failure down. Never claim a test passed that you did not see pass. State exactly what
   you ran (`cargo fmt --check`, `cargo clippy ...`, `cargo test ...`) and what you could not run.
5. Do not touch files outside the scope listed in the task. Tests under `tests/` and
   `crates/*/tests/` belong to the test step, `crates/*/src` to the implementation step. If you
   do both, keep them in separate commits.
6. Three attempts per task. After that mark it blocked in the report, with the reason,
   and move on.
7. Anything that needs a decision by the owner (design, architecture, license, new
   dependency, anything irreversible) is not yours to decide. Write it under "Questions" in
   the report and continue with another task.

## 3. Hand-over: the report

When a task is finished (or blocked), add one file `tasks/reports/<task-id>.md` in your
branch as the last commit, then push the branch:

```
# <task-id> report

Status: done | partly done | blocked
Branch: glm/<task-id>
Last commit: <hash>
Base: <hash of the commit you branched from>

## What changed
- files and one line each

## Verification
- commands you ran and their result, commands you could not run and why

## Deviations from the task
- anything you did differently, and why

## Questions
- decisions for the owner

## Risks / things the reviewer should look at first
- ...
```

The reviewer reads this report first, fetches your branch, runs the full checks in an
isolated worktree, fixes defects, and decides. The owner merges. Do not start the next
task on top of an unreviewed one unless the next task says so.

## 4. Order of work

Work through `tasks/TASKS.md` from top to bottom. Tasks marked **blocked-by-owner** are
not to be started. Do not stop after one task: take the next one, one branch each.
