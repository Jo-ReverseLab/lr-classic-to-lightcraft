# CI proposal

Status: proposal only. This document does not change anything under `.github/`.
Every YAML snippet below is a proposal to be applied by the owner or the lead.

Base: `agent/t22-assumption-triage` (`2214630`), 2026-11-04.

Sources read for this proposal:

- `.github/workflows/ci.yml`
- `PLAN.md`, `README.md`, `CHANGELOG.md`
- `tasks/README.md`, `tasks/reports/t20-performance.md`
- `crates/lr2lc/tests/performance.rs`, `crates/lr2lc/tests/performance_guard.rs`
- one external page: GitHub Actions billing
  (<https://docs.github.com/en/billing/concepts/product-billing/github-actions>,
  retrieved 2026-11-04)
- one external page: Rust 1.85.0 release notes
  (<https://github.com/rust-lang/rust/releases/tag/1.85.0>, retrieved 2026-11-04)

## 1. What CI does today

`ci.yml` below means `.github/workflows/ci.yml`. The whole workflow is 28
lines:

- It runs on every `push` and every `pull_request` (`ci.yml:3-5`) with
  `contents: read` permissions (`ci.yml:7-8`).
- One matrix job on `ubuntu-latest`, `macos-latest` and `windows-latest` with
  `fail-fast: false` (`ci.yml:13-17`).
- Each job checks out the repository, installs the current `stable` toolchain
  with `rustfmt` and `clippy` (`ci.yml:19-22`), then runs
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings` and `cargo test --workspace` (`ci.yml:23-28`).

Not checked today: `--locked` builds, a minimum Rust version, dependency
advisories, coverage, documentation links and prose, a release gate, and
personal data in the diff. `PLAN.md:101-103` lists the same gaps as P4 item 15.

`Cargo.lock` is committed at the repository root, but the workflow does not
pass `--locked`, and the toolchain is `stable` (`ci.yml:20`), so the tested
compiler moves over time.

## 2. Cost model

All figures below are **billed Actions minutes**: Linux wall-clock minutes
count 1×, Windows about 2×, macOS about 10×. This is the unit `PLAN.md` uses.

| Input | Value | Source |
| --- | --- | --- |
| Last full matrix run on main (2026-10-28) | ubuntu passed in 6m34s, windows failed in 5m4s, macos cancelled after 15m | `PLAN.md:9-11` |
| Full matrix cost | roughly 170 Actions minutes | `PLAN.md:38-39` |
| Private-repo rates (2026-11-04) | Linux $0.006/min, Windows $0.010/min, macOS $0.062/min | GitHub billing page, retrieved 2026-11-04 |
| Relative multiplier | Windows ≈ 1.7×, macOS ≈ 10.3× Linux | derived from the rates |
| Check against PLAN | 6.57 + 1.7×5.07 + 10.3×15 ≈ 170 | estimate; consistent with `PLAN.md:38-39` |

Consequences:

- One full matrix run costs about **170 billed minutes ≈ $1.02** (estimate,
  from the observed wall times and the rates above). The macOS figure is a
  lower bound because that job was cancelled, and `PLAN.md:17-19` records that
  the reason (runner availability or a manual cancel) is unresolved.
- Because the workflow listens to both `push` and `pull_request`
  (`ci.yml:3-5`), a push to a branch with an open pull request starts two runs
  of the same matrix: about **340 billed minutes ≈ $2.04 per update**
  (inference from GitHub event semantics, not measured).
- The macOS job dominates every number: it is about 150 of the 170 billed
  minutes. Linux-only additions are rounding errors by comparison.
- `PLAN.md:121-122` warns that Actions minutes are paid and that CI silently
  stops proving anything when the allowance runs out. The plan and the monthly
  allowance are not documented in the repository (owner question).

The monthly scenarios below use **40 CI runs per month** purely as an example
(20 branch updates × 2 because of the double trigger). That number is a
guess, not a measurement; the real volume is not in the repository.

## 3. Candidates

Each candidate lists benefit, run time, cost, risk and the exact change as a
snippet. Runtime figures not taken from the repository are **estimates**.

### C1 – `--locked` builds

**Benefit.** CI builds exactly the committed `Cargo.lock`, so a stale lockfile
or an unreviewed dependency update fails instead of silently resolving new
versions. Reproducible builds are a precondition for the "ready to publish"
claim (`CLAUDE.md:21`, principle 11).

**Run time.** No measurable change; cargo already reads the lockfile.

**Cost.** 0 extra billed minutes.

**Risk.** A dependency update now needs the lockfile in the same commit — that
is the intent. `cargo fmt` has no `--locked` flag, so only clippy and test
change.

**Exact change** (replace `ci.yml:25-28`):

```yaml
      - name: Clippy
        run: cargo clippy --workspace --all-targets --locked -- -D warnings
      - name: Test
        run: cargo test --workspace --locked
```

### C2 – Minimum supported Rust version (MSRV)

**Benefit.** Turns "Rust stable" (`CLAUDE.md:20`, principle 10) into a concrete
promise and catches accidental use of an API newer than the declared version.
The workspace uses `edition = "2024"` (`Cargo.toml:7`), which is stable since
Rust 1.85.0 (Rust 1.85.0 release notes, retrieved 2026-11-04), so the MSRV
cannot be lower than 1.85.0. No `rust-version` is declared in any
`Cargo.toml` today (grep over the workspace), so nothing is promised yet.

**Run time.** One Linux job with a cold build and
`cargo check --workspace --all-targets --locked`: about 4–6 minutes
(estimate; the full ubuntu job took 6m34s, `PLAN.md:9`).

**Cost.** About 4–6 billed minutes per run, about 160–240 billed minutes per
month at 40 runs (estimate).

**Risk.** A second compiler to maintain; a feature that is stable but newer
than the MSRV breaks the job. Someone must decide whether the MSRV covers
tests (`--all-targets`) or only the shipped library and binary. Edition 2024
already forces a recent compiler, so the practical window is small.

**Exact change** (new job in `ci.yml`):

```yaml
  msrv:
    name: MSRV (1.85.0)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85.0
      - name: Check
        run: cargo check --workspace --all-targets --locked
```

The matching code change is `rust-version = "1.85"` in
`[workspace.package]` and `rust-version.workspace = true` in every crate — a
separate change outside `.github/`.

### C3 – Dependency advisories (`cargo audit` or `cargo deny`)

**Benefit.** Reports dependency versions with known advisories. The allowed
dependency set is deliberately small (`tasks/README.md:52-54`), so the signal
is short and actionable. `cargo deny` can also enforce the license rule
(MIT OR Apache-2.0) and an allowlist of crates.

**Run time.** About 1–2 minutes with a prebuilt action (estimate); installing
`cargo-audit` from source would add several minutes and should be cached or
installed through an action.

**Cost.** About 1–2 billed minutes per run on Linux; as a weekly schedule
about 4–8 billed minutes per month (estimate).

**Risk.** This needs a **new third-party tool or action and the owner's
approval**, as the task text says. The advisory database is fetched over the
network. Advisories for unmaintained crates can turn into red CI even when the
project is not affected. The workflow-level `contents: read` permission
(`ci.yml:7-8`) is not enough for actions that open issues; prefer report-only,
or grant the narrow extra permission deliberately.

**Exact change** (weekly job, plain `cargo audit`, report-only):

```yaml
  audit:
    name: cargo audit
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@cargo-audit
      - run: cargo audit

# add to the top-level `on:` block
#   schedule:
#     - cron: '0 3 * * 1'
```

`cargo deny` would replace the two steps with `EmbarkStudios/cargo-deny-action@v2`
and a `deny.toml`; both are third-party and need the same approval.

### C4 – Coverage report

**Benefit.** Shows which reader, planner and writer paths the tests never
execute. Useful before the MVP release and for the three-part split
(`CLAUDE.md:17`, principle 7). Not a correctness gate by itself.

**Run time.** `cargo llvm-cov --workspace` instruments the whole suite: about
6–9 minutes on Linux (estimate; the uninstrumented test job took 6m34s,
`PLAN.md:9`).

**Cost.** About 6–9 billed minutes per run on Linux; weekly about 24–36 per
month; daily on main about 180–270 per month (estimates).

**Risk.** Third-party tool (`cargo-llvm-cov`) and install action. Uploading to
an external service such as Codecov would send repository data to a third
party and must not be enabled without an explicit owner decision. Coverage
percentages invite gaming and say nothing about the quality of the assertions.

**Exact change** (weekly, no upload):

```yaml
  coverage:
    name: Coverage
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: taiki-e/install-action@cargo-llvm-cov
      - run: cargo llvm-cov --workspace --locked --summary-only
```

Add the same top-level `schedule:` trigger as in C3.

### C5 – Performance guard on a schedule

**Benefit.** The 2,000-photo guard already runs in the normal suite on every
push (`crates/lr2lc/tests/performance_guard.rs:33-34`, limit 180 s at
`:31`). The expensive 10,000 / 100,000 / 500,000-photo measurements are
`#[ignore]`d (`crates/lr2lc/tests/performance.rs:44-58`), so a regression that
appears only at scale is not seen. T20 measured 13.0 s for the 500,000-photo
dry run after the fix (`tasks/reports/t20-performance.md:84-88`) and about one
minute including catalog generation (`tasks/reports/t20-performance.md:89-92`).

**Run time.** A release build (about 2–4 minutes, estimate) plus the three
measurements (about 1–2 minutes, estimate, based on the T20 numbers; the CI
runner may differ from the Pi).

**Cost.** About 3–6 billed minutes per weekly run, about 13–26 per month
(estimate).

**Risk.** Shared runners are noisy, so absolute times are not comparable with
the Pi measurements. The ignored tests deliberately have no time limit
(`crates/lr2lc/tests/performance.rs:22-24`); a scheduled job would only print
numbers unless a threshold is chosen (owner decision). The 500,000-photo
catalog writes 500,000 placeholder files into a temporary directory — disk
space and setup time are real on CI.

**Exact change** (weekly; keep the 2,000-photo guard where it is):

```yaml
  performance:
    name: Performance measurements
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: >-
          cargo test --release -p lr2lc --test performance --locked --
          --ignored --nocapture
```

Add the same top-level `schedule:` trigger as in C3.

**Recommendation.** Do not move the 2,000-photo guard out of the normal
suite: it costs a few seconds and already catches accidental quadratic work
on every push (`crates/lr2lc/tests/performance_guard.rs:4-7`). Move only the
measurement runs to the schedule.

### C6 – Link and prose checks on README and CHANGELOG

**Benefit.** `README.md` has no external Markdown links (grep for `](http`
returns zero), only relative links such as `LICENSE-APACHE` and `LICENSE-MIT`
(`README.md:667-668`) and in-page anchors; `CHANGELOG.md` has one external
link (`CHANGELOG.md:5`). A relative-file check catches a missing license file
or a renamed document. A prose check (vale) could enforce English and the
"Lightroom as compatibility statement only" rule (`CLAUDE.md:21`, principle
11), but no style guide exists yet.

**Run time.** Relative-file check under 10 seconds; lychee (external links)
about 30–60 seconds plus network variance; vale about 30–60 seconds with a
style guide.

**Cost.** In-repository script: 0 extra billed minutes inside the Linux job.
Each external tool: about 1–2 billed minutes per run (estimate).

**Risk.** External link checkers fail on rate limits and transient errors;
vale needs a maintained style guide and produces noise. Both need new
third-party actions. The value is low right now because there are almost no
external links.

**Exact change** (no new tool; checks relative paths only):

```yaml
      - name: Check documented relative links
        run: |
          for f in README.md CHANGELOG.md; do
            grep -oE '\]\([^)#]+\)' "$f" | sed 's/^](//;s/)$//' |
            while read -r p; do
              case "$p" in http*|"") continue ;; esac
              test -e "$p" || { echo "$f: missing $p"; exit 1; }
            done
          done
```

This does not check `#anchors` or links inside fenced code blocks, and it is
not a substitute for a real link checker.

### C7 – `tasks/` gone before a release

**Benefit.** `tasks/` is a working channel and is removed before the first
public release (`tasks/README.md:8-9`); principle 11 says the project is
"ready to publish from the first commit" (`CLAUDE.md:21`). A tag-gated job makes
the release fail while the directory is still present instead of relying on
memory.

**Run time.** Under 5 seconds.

**Cost.** About 0 per push (the job runs only on tags); one run per release.

**Risk.** It must not run on every push, or it fails today because `tasks/`
exists. The project has no releases or tags yet (`CHANGELOG.md:6`), so the
release trigger must be decided. The check proves only that the directory is
gone; it does not prove that no private data remains in history (see C8).

**Exact change:**

```yaml
  release-sanity:
    name: Release sanity
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: tasks/ must be gone before a release
        run: test ! -e tasks
```

### C8 – Personal paths and names in the diff

**Benefit.** Enforces `CLAUDE.md:21` (principle 11) and the rule "No personal
paths, user names, host names, e-mail addresses" (`tasks/README.md:48-49`).
The repository is intended to become public; a leak is cheap to catch in the
diff and expensive after publication.

**Run time.** Under 10 seconds.

**Cost.** 0 extra billed minutes as a step in the Linux job.

**Risk.** False positives are real and were measured here: `/Users/example/...`
is an intentional neutral placeholder in the README examples
(`README.md:209-211`) and in many tests (for example
`crates/lc-writer/tests/apply_albums.rs:76`); `tasks/TASKS.md:381` explicitly
asks for neutral paths like `C:\data\...`. A scan over `git log --all -p`
found no e-mail addresses in added lines (0 matches), but it did find the
intentional `/Users/example` occurrences. A naive check would fail on correct
content. There is also an existing name in the repository: `PLAN.md:124`
names the repository owner/organisation in a `gh api` example — the owner
should review that line separately. Finally, a pattern scan is easy to evade
by splitting a string; it reduces accidents, it does not prove absence.

**Exact change** (pull requests only; allowlist the documented placeholders):

```yaml
      - name: Scan added lines for personal data
        if: github.event_name == 'pull_request'
        run: |
          git diff --unified=0 \
            "origin/${{ github.event.pull_request.base.ref }}...HEAD" -- . \
            | grep -E '^\+' | grep -vE '^\+\+\+' > added.txt || true
          if grep -nE \
            '/(home|Users)/[A-Za-z0-9._-]+|C:\\Users\\|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
            added.txt | grep -vE '/Users/example|/home/example'; then
            echo "possible personal data in the diff"
            exit 1
          fi
```

The allowlist filters whole lines, so a line that contains both a placeholder
and a real path would be missed; split the check per match if that matters.
The checkout must use `fetch-depth: 0` (or fetch the base ref) for the diff
to work:

```yaml
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
```

### C9 – Build the three systems with a cache

**Benefit.** The matrix already builds on all three systems
(`ci.yml:15-17`). Caching the cargo registry and build artifacts shortens
repeated runs, which matters most on macOS because it bills about 10×. A
separate fourth "build" job would duplicate the test job and add cost; the
right place is the existing matrix.

**Run time.** First run unchanged; later runs shorter by an unmeasured amount
(estimate: 40–60 % of compile time).

**Cost.** 0 extra minutes. Expected saving (estimate, not measured): about
2–4 billed minutes per Linux run, about 2–4 per Windows run, about 5–10 per
macOS run.

**Risk.** Third-party action (pin it to a commit SHA). Stale caches after a
toolchain change — the action keys the cache on `Cargo.lock` and the
toolchain, but a `shared-key` must include the OS. The cache limit is 10 GB
per repository and macOS/Windows caches are large. A poisoned cache can hide a
missing build step; `--locked` (C1) and a periodic key change reduce that.
`Swatinem/rust-cache` was not tested in this environment.

**Exact change** (after the toolchain step in the `test` job,
`ci.yml:19-22`):

```yaml
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: ${{ matrix.os }}
          save-if: ${{ github.ref == 'refs/heads/main' }}
```

### C10 – Remove duplicate runs and cancel superseded ones

Not in the task's candidate list, but it changes the cost of every other
candidate, so it is ranked first in section 4.

**Benefit.** A push to a branch with an open pull request currently starts the
full matrix twice (`ci.yml:3-5`). Restricting `push` to `main` and adding a
concurrency group halves the cost of every branch update and cancels runs
that a newer push has superseded.

**Run time.** Unchanged per run.

**Cost.** Saves about 170 billed minutes per branch update (estimate: half of
about 340).

**Risk.** A direct push to a non-main branch without a pull request gets no
CI. If branch pushes must keep running on their own, keep `push:` bare and
add only `concurrency`. `cancel-in-progress` cancels superseded runs, which is
desirable here but would be wrong if another workflow depended on this one
(none does today).

**Exact change** (top of the workflow):

```yaml
on:
  push:
    branches: [main]
  pull_request:

concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

## 4. Ranked list and recommendation

Ranking by benefit per billed minute, with the risk above:

| Rank | Candidate | Benefit | Extra cost per run | Risk | Recommendation |
| --- | --- | --- | --- | --- | --- |
| 1 | C10 no duplicate runs | halves the bill of every update | saves ≈170 billed min | low; trigger policy | adopt now |
| 2 | C1 `--locked` | reproducible builds, stale lockfile fails | 0 | very low | adopt now |
| 3 | C9 cache | cuts compile time, biggest on macOS | saves ≈2–10 billed min | third-party action, stale cache | adopt after C10 |
| 4 | C7 `tasks/` release gate | protects the first release | ≈0 | tag convention | adopt now |
| 5 | C8 personal-data scan | enforces principle 11 | 0 | false positives, allowlist needed | adopt with allowlist |
| 6 | C2 MSRV | concrete compiler promise | ≈4–6 billed min (Linux) | version choice, maintenance | adopt after the owner picks 1.85 |
| 7 | C5 scheduled measurements | catches scale regressions | ≈3–6 billed min/week | noise, no threshold | adopt weekly |
| 8 | C3 cargo audit | advisory and license signal | ≈1–2 billed min/week | new tool, owner approval | adopt once approved |
| 9 | C6 links/prose | catches broken docs | ≈0 for the script | third-party tools, noise | adopt the script; defer lychee/vale |
| 10 | C4 coverage | shows untested paths | ≈6–9 billed min (Linux) | vanity metric, no upload | defer until after the release |

**Recommendation.**

1. Adopt C10, C1, C7 and C8 first. They cost nothing extra, and C10 alone
   saves about 170 billed minutes per branch update.
2. Add the cache (C9) next; it is the only change that reduces the dominant
   macOS cost.
3. Add the weekly performance measurements (C5) and the weekly audit (C3)
   once the owner approves the tools.
4. Add the MSRV job (C2) only after the owner declares a version; 1.85 is the
   floor because of edition 2024.
5. Defer coverage (C4) and the external link/prose tools (C6). The in-repo
   relative-link check can go in immediately at no cost.

If only the zero-cost items are adopted (C1, C7, C8, C10, plus the C6 script),
the monthly bill drops instead of rising. With C2, C3, C4 and C5 all enabled,
the added Linux cost stays around 2.5–4 USD per month at the 40-run scenario
(estimate: 416–632 billed Linux minutes at $0.006); the macOS minutes remain
the dominant cost.

## 5. Decisions for the owner

1. **Trigger policy (C10).** May `push` be restricted to `main`? If branch
   pushes without a pull request must keep running, only `concurrency` can be
   added.
2. **Budget.** What is the monthly Actions allowance, and is 40 runs per
   month realistic? The repository does not document either (`PLAN.md:121-122`).
3. **Third-party tools/actions.** May `cargo-audit`, `cargo-llvm-cov`,
   `Swatinem/rust-cache`, lychee and vale be introduced, and must every action
   be pinned to a commit SHA? The repository already uses `dtolnay/rust-toolchain`
   (`ci.yml:20`), but each new tool needs approval under the project rules.
4. **MSRV.** Declare 1.85.0 (the edition-2024 floor), a newer version, or none
   at all? Should `rust-version` be added to every crate?
5. **Advisories.** `cargo audit` or `cargo deny`? Weekly or per push? Block
   the merge or only report? May the action open issues?
6. **Coverage.** Ever, or not? Weekly or per push? No upload to an external
   service without a separate decision.
7. **Performance threshold.** Should the scheduled measurements fail on a
   threshold, or only log numbers? Which threshold?
8. **Release trigger (C7).** Which tags start a release (`v*`?), and should
   the release job check more than `tasks/` (version in `Cargo.toml`,
   `CHANGELOG.md`)?
9. **Existing name (C8).** `PLAN.md:124` names the repository
   owner/organisation in a `gh api` example. Should that line be edited before
   the personal-data scan is enabled?
10. **Prose style (C6).** No style guide exists. Is vale wanted, and who
    maintains the guide?

## Open questions

- Is the repository on a plan where every Actions minute is paid, or are
  included minutes available? The repository only says "Actions minutes are
  paid" (`PLAN.md:121-122`).
- Was the 15-minute macOS run a manual cancel or a runner shortage?
  `PLAN.md:17-19` records both possibilities; the cost estimate assumes a
  normal 15-minute job and is therefore a lower bound.
- Does the GitHub organisation/repository policy allow third-party actions,
  and does it require SHA pinning?
- What is the real CI volume per month? The 40-run scenario is a guess.
- `cargo test --workspace` does not use `--all-targets`, while clippy does
  (`ci.yml:25-28`). Is that intentional, or are some test targets never
  compiled by the test step?
- `Swatinem/rust-cache`, `cargo-llvm-cov`, `cargo-audit` and `lychee` were not
  exercised on this machine (Raspberry Pi, no C toolchain; `PLAN.md:24-29`),
  so their run times and cache behaviour are estimates.

## Recommendation

Adopt the zero-cost, low-risk checks now (C10, C1, C7, C8, and the in-repo
link script from C6), add the cache (C9), then the weekly performance and
audit jobs (C5, C3) once the owner approves the tools. Decide the MSRV before
adding C2, and defer coverage and external prose/link tooling (C4, the rest of
C6) until after the first release. No change under `.github/` is part of this
document; all snippets are proposals for the owner or the lead to apply.
