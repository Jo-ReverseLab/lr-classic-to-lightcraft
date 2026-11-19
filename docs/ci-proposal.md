# CI proposal

> Historical (2026-11-04). The cost figures below assume a private
> repository. Since the repository is public, standard runners are free and
> every pull request runs the full matrix; the current workflow is
> `.github/workflows/ci.yml`.

Status: proposal only. This document does not change anything under `.github/`.
Every YAML snippet below is a proposal to be applied by the owner or the lead.

Base: `agent/t22-assumption-triage` (`2214630`), 2026-11-04. `PLAN.md` was
later replaced by `ROADMAP.md`; references to `PLAN.md` lines below describe
that earlier state.

Sources read for this proposal:

- `.github/workflows/ci.yml`
- `PLAN.md`, `README.md`, `CHANGELOG.md`
- `tasks/README.md`, `tasks/reports/t20-performance.md`
- `crates/lr2lc/tests/performance.rs`, `crates/lr2lc/tests/performance_guard.rs`
- one external page: GitHub Actions billing
  (<https://docs.github.com/en/billing/concepts/product-billing/github-actions>,
  retrieved and read 2026-11-04; the per-minute rates quoted below match the
  table on that page)
- two external pages: Rust 1.85.0 release notes
  (<https://github.com/rust-lang/rust/releases/tag/1.85.0>, retrieved 2026-11-04)
  for the edition-2024 floor, and Rust 1.88.0 release notes
  (<https://github.com/rust-lang/rust/releases/tag/1.88.0>, retrieved
  2026-11-05) for `slice::as_chunks` and let-chains
- `cargo-audit` source, for the exit codes of `cargo audit`
  (<https://github.com/rustsec/rustsec/blob/main/cargo-audit/src/commands/audit.rs>,
  read 2026-11-04)

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
personal data in the diff. This list overlaps with P4 item 15 (`PLAN.md:101-103`),
but the two are not identical in either direction: PLAN also names the `large`
fixture as a performance guard and a real LightCraft app, which this list does
not; PLAN does not name a release gate or a personal-data scan, which this list
adds.

`Cargo.lock` is committed at the repository root, but the workflow does not
pass `--locked`, and the toolchain is `stable` (`ci.yml:20`), so the tested
compiler moves over time.

## 2. Cost model

Two different units appear below and must not be mixed:

- **Billed Actions minutes** — GitHub's billing unit. A Linux wall-clock minute
  counts 1, Windows ≈ 1.67 and macOS ≈ 10.33 (rounded to "about 2×" and
  "about 10×" in prose). This is the unit `PLAN.md:38-39` uses.
- **Dollar rates** — Linux $0.006/min, Windows $0.010/min, macOS $0.062/min
  for standard 2-core runners on a private repository.

The billed-minute weights are the dollar rates divided by the Linux rate
(0.010 / 0.006 ≈ 1.67; 0.062 / 0.006 ≈ 10.33). They are therefore ratios
derived from the rates, not independent measurements. Because of that, one
billed (Linux-equivalent) minute costs $0.006, and multiplying billed minutes
by $0.006 gives the dollar figure. All dollar figures in this document apply
only while the repository is private: the billing page states that standard
GitHub-hosted runners are free in public repositories.

| Input | Value | Source |
| --- | --- | --- |
| Last full matrix run on main (2026-10-28) | ubuntu passed in 6m34s, windows failed in 5m4s, macos cancelled after 15m | `PLAN.md:9-11` |
| Full matrix cost | roughly 170 billed minutes | `PLAN.md:38-39` |
| Private-repo rates (2026-11-04), standard 2-core runners | Linux $0.006/min, Windows $0.010/min, macOS $0.062/min | GitHub billing page, retrieved and read 2026-11-04 |
| Billed-minute weight (rounded prose form) | Windows ≈ 1.67× (≈ 2×), macOS ≈ 10.33× (≈ 10×) Linux | derived from the rates above |
| Check against PLAN | 6.57 + 1.67×5.07 + 10.33×15 ≈ 170 | estimate; consistent with `PLAN.md:38-39` |

Consequences:

- One full matrix run costs about **170 billed minutes ≈ $1.02** (estimate,
  from the observed wall times and the rates above). The macOS figure is a
  lower bound because that job was cancelled, and `PLAN.md:17-19` records that
  the reason (runner availability or a manual cancel) is unresolved.
- Because the workflow listens to both `push` and `pull_request`
  (`ci.yml:3-5`), a push to a branch with an open pull request starts two runs
  of the same matrix: about **340 billed minutes ≈ $2.04 per update**
  (inference from GitHub event semantics, not measured).
- The macOS job dominates every number: at the derived weight it is about 155
  of the 170 billed minutes. Linux-only additions are rounding errors against
  it, but they are still real dollars (see the table in section 4).
- `PLAN.md:121-122` warns that Actions minutes are paid and that CI silently
  stops proving anything when the allowance runs out. The plan and the monthly
  allowance are not documented in the repository (owner question).

Monthly figures are given for two explicitly stated scenarios; the frequency
matters because it changes every sum:

- **S1 – 40 CI runs per month**: 20 branch updates × 2 runs each, because the
  workflow listens to both `push` and `pull_request` (`ci.yml:3-5`). This is
  the baseline **before C10**.
- **S2 – 20 CI runs per month**: the same 20 branch updates **after C10**
  removes the duplicate trigger.
- **Weekly jobs** (C3, C4 on its proposed weekly schedule, C5) are counted as
  **4 runs per month**. C4 "daily on main" is counted as **30 runs per month**.
- Per-push jobs (C2) scale with S1 or S2. Weekly and daily jobs do not scale
  with them.

The 20 branch updates are a guess, not a measurement; the real volume is not
in the repository. All figures are Linux billed minutes at $0.006/min.

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
cannot be lower than 1.85.0. The code, however, needs more than that floor:
`crates/lr-reader/src/develop.rs` uses `slice::as_chunks`, and let-chains appear
in `crates/lc-writer/src/apply.rs` and `crates/lr2lc/tests/collections_report.rs`.
Both are stable since Rust 1.88.0 (Rust 1.88.0 release notes, retrieved
2026-11-05; let-chains are stable in the 2024 edition). The MSRV promise is
therefore 1.88.0, declared as `rust-version = "1.88.0"` in
`[workspace.package]` (`Cargo.toml:8`).

**Run time.** One Linux job with a cold build and
`cargo check --workspace --all-targets --locked`: about 4–6 minutes
(estimate; the full ubuntu job took 6m34s, `PLAN.md:9`).

**Cost.** About 4–6 billed minutes per run; about 160–240 billed minutes per
month at S1 (40 runs) or 80–120 at S2 (20 runs) (estimates).

**Risk.** A second compiler to maintain; a feature that is stable but newer
than the MSRV breaks the job. Someone must decide whether the MSRV covers
tests (`--all-targets`) or only the shipped library and binary. Edition 2024
already forces a recent compiler, so the practical window is small.

**Exact change** (new job in `ci.yml`):

```yaml
  msrv:
    name: MSRV (1.88.0)
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262  # v4.4.0
      - name: Toolchain
        uses: dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8  # master
        with:
          toolchain: 1.88.0
      - name: Check
        run: cargo check --workspace --all-targets --locked
```

The matching code change is the value in `[workspace.package]`:
`rust-version = "1.88.0"` (`Cargo.toml:8`). Every crate already inherits it
with `rust-version.workspace = true`, so no crate manifest changes.

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
network. Advisories for unmaintained crates can create noise even when the
project is not affected; with the `continue-on-error` below they mark the step
in the UI but do not turn the run red. The workflow-level `contents: read` permission
(`ci.yml:7-8`) is not enough for actions that open issues; prefer report-only,
or grant the narrow extra permission deliberately.

**Exact change** (weekly job, plain `cargo audit`, report-only):

```yaml
  audit:
    name: cargo audit (report-only)
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@cargo-audit
      - name: cargo audit
        continue-on-error: true
        run: cargo audit

# add to the top-level `on:` block
#   schedule:
#     - cron: '0 3 * * 1'
```

"Report-only" needs one explicit step: plain `cargo audit` exits with **1**
when it finds vulnerabilities or denied findings and with **2** on operational
errors (`cargo-audit/src/commands/audit.rs`, `should_exit_with_failure` and
`exit(1)`/`exit(2)`; source read 2026-11-04). Without `continue-on-error` the
job would turn red on every advisory, which is the opposite of reporting only.
With `continue-on-error: true` the step is marked failed in the UI and keeps
its log, but the job and the workflow run stay green; the scheduled run is not
a merge gate. If the owner wants a gate instead, delete `continue-on-error`
(candidate for section 5, decision 5).

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

**Cost.** About 3–6 billed minutes per weekly run, about 12–24 per month
(estimate; 4 runs per month, see section 2).

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
(`README.md:713-714`) and in-page anchors; `CHANGELOG.md` has one external
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
2–4 billed minutes for the Linux run, about 2–4 for the Windows run and about
5–10 for the macOS run — roughly **9–18 billed minutes per full matrix**
(these are billed minutes as defined in section 2, so they already include the
per-OS weight).

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
about 340), i.e. at S1's 20 branch updates about **3,400 billed minutes ≈
$20 per month** while the repository is private.

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

| Rank | Candidate | Benefit | Cost effect per trigger (estimate) | Risk | Recommendation |
| --- | --- | --- | --- | --- | --- |
| 1 | C10 no duplicate runs | halves the bill of every update | saves ≈170 billed min per branch update | low; trigger policy | adopt now |
| 2 | C1 `--locked` | reproducible builds, stale lockfile fails | 0 | very low | adopt now |
| 3 | C9 cache | cuts compile time, biggest on macOS | saves ≈9–18 billed min per matrix run | third-party action, stale cache | adopt after C10 |
| 4 | C7 `tasks/` release gate | protects the first release | ≈0 (tags only) | tag convention | adopt now |
| 5 | C8 personal-data scan | enforces principle 11 | 0 (inside the Linux job) | false positives, allowlist needed | adopt with allowlist |
| 6 | C2 MSRV | concrete compiler promise | ≈4–6 billed min per run | version choice, maintenance | adopt with 1.88.0 |
| 7 | C5 scheduled measurements | catches scale regressions | ≈3–6 billed min per weekly run | noise, no threshold | adopt weekly |
| 8 | C3 cargo audit | advisory and license signal | ≈1–2 billed min per weekly run | new tool, owner approval | adopt once approved |
| 9 | C6 links/prose | catches broken docs | ≈0 for the script | third-party tools, noise | adopt the script; defer lychee/vale |
| 10 | C4 coverage | shows untested paths | ≈6–9 billed min per run | vanity metric, no upload | defer until after the release |

**Recommendation.**

1. Adopt C10, C1, C7 and C8 first. They cost nothing extra, and C10 alone
   saves about 170 billed minutes per branch update.
2. Add the cache (C9) next; it is the only change that reduces the dominant
   macOS cost.
3. Add the weekly performance measurements (C5) and the weekly audit (C3)
   once the owner approves the tools.
4. Add the MSRV job (C2) with 1.88.0: edition 2024 sets the floor at 1.85.0,
   and the code uses `slice::as_chunks` (`crates/lr-reader/src/develop.rs`) and
   let-chains (`crates/lc-writer/src/apply.rs`,
   `crates/lr2lc/tests/collections_report.rs`), stable since 1.88.0.
5. Defer coverage (C4) and the external link/prose tools (C6). The in-repo
   relative-link check can go in immediately at no cost.

If only the zero-cost items are adopted (C1, C7, C8, C10, plus the C6 script),
the monthly bill drops instead of rising. The paid checks depend on frequency,
so the arithmetic is spelled out rather than compressed into one headline
figure. All values are billed Linux minutes at **S1 (40 runs/month)**; weekly
jobs count 4 runs, C4 daily on main counts 30 runs:

| Set | C2 (per run) | C3 (weekly) | C4 (coverage) | C5 (weekly) | Total billed min/month | ≈ USD/month |
| --- | --- | --- | --- | --- | --- | --- |
| Recommended: C2 + C3 + C5, C4 deferred | 160–240 | 4–8 | — | 12–24 | 176–272 | 1.06–1.63 |
| All four, C4 weekly (as proposed in C4) | 160–240 | 4–8 | 24–36 | 12–24 | 200–308 | 1.20–1.85 |
| All four, C4 daily on main | 160–240 | 4–8 | 180–270 | 12–24 | 356–542 | 2.14–3.25 |

After C10 (**S2, 20 runs/month**) the C2 line halves: the recommended set costs
96–152 billed minutes ≈ $0.58–0.91; all four with C4 weekly costs 120–188
≈ $0.72–1.13. An earlier draft of this section said "416–632 billed minutes,
2.5–4 USD" for C2 + C3 + C4 + C5; that silently assumed C4 on all 40 runs,
which contradicts both the weekly proposal in C4 and its deferral in the
recommendation above. The table replaces that figure.

For scale, the existing matrix at S1 (40 runs × ≈170 billed minutes) is about
**6,800 billed minutes ≈ $41/month** while private, uncached and before C10;
C10 alone removes half of it (≈3,400 billed minutes ≈ $20/month at 20 branch
updates). The added Linux jobs above are therefore small next to the matrix,
and the macOS share of it (≈155 of 170 billed minutes per run) remains the
dominant cost. If the repository becomes public, standard runners are free
(the billing page), so this whole model describes the private phase only.

## 5. Decisions for the owner

1. **Trigger policy (C10).** May `push` be restricted to `main`? If branch
   pushes without a pull request must keep running, only `concurrency` can be
   added.
2. **Budget.** What is the monthly Actions allowance, and is S1 (40 runs per
   month) or S2 (20, after C10) realistic? The repository does not document
   either (`PLAN.md:121-122`).
3. **Third-party tools/actions.** May `cargo-audit`, `cargo-llvm-cov`,
   `Swatinem/rust-cache`, lychee and vale be introduced, and must every action
   be pinned to a commit SHA? The repository already uses `dtolnay/rust-toolchain`
   (`ci.yml:20`), but each new tool needs approval under the project rules.
4. **MSRV.** Decided on 2026-11-05: declare 1.88.0. Edition 2024 sets the floor
   at 1.85.0, but the code uses `slice::as_chunks` and let-chains, both stable
   since 1.88.0. `rust-version` lives in `[workspace.package]` and every crate
   inherits it, so no crate manifest needs its own value. Raised to 1.89.0
   on 2026-11-19, because rusqlite 0.43 requires it.
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
  paid" (`PLAN.md:121-122`). The billing page also states that standard hosted
  runners are free in public repositories; the cost model here therefore
  applies only while the repository is private.
- Was the 15-minute macOS run a manual cancel or a runner shortage?
  `PLAN.md:17-19` records both possibilities; the cost estimate assumes a
  normal 15-minute job and is therefore a lower bound.
- Does the GitHub organisation/repository policy allow third-party actions,
  and does it require SHA pinning?
- What is the real CI volume per month? S1 (40 runs) and S2 (20 runs) are
  guesses.
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
