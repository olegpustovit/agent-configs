# Fast iteration: deadlines, incremental builds, and test tiers

Use this reference before running tests, and before changing profiles, linkers, crate boundaries, test layout, or CI caches. Runtime performance of the service itself is in [allocation.md](allocation.md).

Fast compilation and fast tests are requirements, with numbers. A slow test or a slow rebuild is a defect to fix, not a cost to wait out.

## Contents

- The `cargo t` wrapper
- Test tiers and deadlines
- The edit loop
- Build-time budgets
- Dev profile
- Keep incremental builds incremental
- Writing tests that fit the deadlines
- Release profile
- CI

## The `cargo t` wrapper

Every test run goes through `cargo t`. Never run raw `cargo test` or a test executable directly: nothing stops those from hanging or quietly growing slow.

Install it once per workspace:

1. Copy [assets/t](../assets/t) to `tools/t/`. It uses only the standard library and builds in about a second.
2. Add the alias to `.cargo/config.toml`:

```toml
[alias]
t = "run -q --manifest-path tools/t/Cargo.toml --target-dir target/t-tool --"
```

What it does:

- Builds the test executables once with the normal shared `target/` cache.
- Runs every test case as its own process, in parallel, under a hard deadline. A case that overruns is killed with its whole process group and reported as `TIMEOUT`.
- Stops the whole run at a run deadline and marks the remainder `NOT RUN`.
- Prints output only for cases that failed or timed out.
- Prints the five slowest cases after every run and appends every case's time to `target/t-times.tsv`.
- Runs cases whose function name starts with `serial_` one at a time, after the parallel batch. Use that prefix for the rare test that needs an exclusive resource.

```sh
cargo t -p core                  # one crate
cargo t -p core rename           # cases whose name contains "rename"
cargo t --workspace              # every crate, still under the run deadline
T_SLOW=1 cargo t --workspace     # qualification tier
```

If the repository already has an equivalent enforced runner, use it. The contract matters, not this implementation: per-case hard kill, run deadline, slowest report, recorded timings.

## Test tiers and deadlines

| Tier | Where | Per-case deadline | Run deadline | When to run |
| --- | --- | --- | --- | --- |
| unit | `#[cfg(test)]` modules under `src/` | 2 s | 60 s | after every edit, focused |
| integration | files under `tests/`, including PostgreSQL repository contracts | 5 s | 60 s | at a stable boundary and before handoff |
| slow | cases marked `#[ignore]`, run with `T_SLOW=1` | 60 s | 600 s | final qualification, explicitly |

Rules:

- The deadlines are fixed. Never raise one, add a retry, or weaken an assertion to make a case pass.
- When a case times out, fix it in this order: shrink the fixture, share setup, remove real sleeps and real network, split the case. Only a test that is exhaustive by nature moves to the slow tier, with `#[ignore = "slow: <reason>"]`.
- A unit test that needs more than a few milliseconds is suspect. Typical unit cases finish in under 10 ms; the 2 s deadline is a ceiling, not a target.
- The slow tier is small. If it grows, the fast tiers are missing coverage that should be cheap.
- A hang is a failure with a cause. Find the missing timeout, the deadlock, or the unbounded loop; do not rerun.

## The edit loop

Stage verification by cost. Do not run the workspace suite after every edit.

| Step | Command | Budget (warm) |
| --- | --- | --- |
| typecheck after an edit | `cargo check -p <crate>` | 1 to 5 s |
| focused regression | `cargo t -p <crate> <filter>` | under 10 s including build |
| touched crates and their consumers, at a stable boundary | `cargo t -p <crate> -p <consumer>` | under 30 s |
| before handoff | `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo t --workspace` | under 60 s of test time |
| final qualification | `T_SLOW=1 cargo t --workspace` | under 600 s |

Report the commands run, wall times, and the slowest cases the wrapper printed. Leave one small runnable regression test for any nontrivial fix.

## Build-time budgets

Measure the incremental rebuild, because that is what an edit costs. The wrapper has a mode for it: it warms the build, bumps one source file's modification time, and times the rebuild.

```sh
cargo t --touch crates/core/src/components/note/service.rs --budget 5 -p api
```

It exits nonzero when the rebuild exceeds the budget and appends the time to `target/build-times.tsv`.

Budgets for a debug rebuild of the main binary after touching one file, on the development machine:

| Touched crate | Budget |
| --- | --- |
| leaf crate with no workspace dependents beyond the binary | 1 s for `cargo check`, 5 s for the binary |
| middle crate such as `core` | 5 s for `cargo check`, 10 s for the binary |
| top binary crate | 5 s |

When a budget is exceeded, fix the cause before continuing: a new heavy dependency or feature, a procedural macro on a hot edit path, a generic function instantiated in many crates, a crate that has grown too large to rebuild quickly. Run `cargo build --timings` to see which crate and which phase is slow. Record budgets and measurements in version control; state the machine.

## Dev profile

Start from this profile and change it only with a before-and-after measurement of both rebuild time and test run time.

```toml
[profile.dev]
opt-level = 0
debug = 0                 # no debug info: faster codegen and link, much smaller target/. Panic locations still print.
incremental = true
codegen-units = 256
overflow-checks = true

[profile.dev.package."*"]
opt-level = 1             # dependencies are compiled once; optimizing them keeps tests fast
debug = 0
```

- Set `debug = "line-tables-only"` temporarily when you need line numbers in a backtrace or a debugger. Do not commit it without measuring the rebuild cost.
- Give a specific dependency `opt-level = 3` when its unoptimized speed dominates test time. Password hashing, compression, and cryptography crates are the usual cases; argon2 at `opt-level = 0` alone can break a 2 s deadline.
- If application code itself is too slow unoptimized for tests to meet their deadlines, raise `[profile.dev] opt-level` to 1 and measure the rebuild cost against the test-time gain.
- Tests use the same settings through `[profile.test]` inheriting from `dev`. Do not give tests a different profile; that compiles everything twice.

## Keep incremental builds incremental

- Keep one warm, shared `target/`. No routine `cargo clean`, no private target directories per task, no per-command `RUSTFLAGS` or profile overrides. Each of those discards the cache.
- Put flags in `.cargo/config.toml` and profiles in `Cargo.toml`, where every command sees the same values.
- Use `cargo check` and clippy while iterating. Build a binary only to run it.
- Keep binary crates thin so a handler edit does not rebuild `core`. Split a crate when its rebuild exceeds its budget, along a real dependency boundary.
- Keep generic code out of widely used signatures when a concrete type does the job; every instantiation is compiled in the crate that uses it.
- Disable unused default features on heavy dependencies. Check that two crates do not enable different feature sets of the same dependency, which builds it twice.
- Treat a new procedural-macro dependency as a build-time cost to justify.
- Build with `SQLX_OFFLINE=true` so compiling never waits on a database.
- Use one integration-test binary per crate: `tests/it/main.rs` with submodules, not many top-level files in `tests/`. Each top-level file is a separate executable to link.
- Set `doctest = false` on library crates that have no documentation examples. Doc tests compile one binary per example and the wrapper does not run them.

Linking dominates incremental rebuilds of a large binary:

- On `x86_64-unknown-linux-gnu`, `rust-lld` is already the default linker.
- On `aarch64-unknown-linux-gnu` it is not. Configure `mold` (or `wild`):

```toml
[target.aarch64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

- The Cranelift backend is still nightly-only. Do not depend on it.

## Writing tests that fit the deadlines

- No real sleeps. Use `#[tokio::test(start_paused = true)]` and advance time, or inject a clock.
- No real network. Fake the outbound client at the consumer-owned trait, or use `wiremock` on loopback.
- Service tests use a hand-written fake repository and never touch the database.
- Repository contract tests use `#[sqlx::test]`, which creates a migrated database per test. Keep fixtures to the rows the assertion needs. If database creation itself threatens the 5 s deadline, the migration set is too slow to apply; squash a baseline for tests.
- Put a timeout inside any test that awaits something that might never arrive, so the failure names the cause before the wrapper has to kill the process.
- Use `insta` snapshots for serialized contracts, and `proptest` with a small case count in the fast tiers. Long property runs and `cargo fuzz` are slow tier or scheduled.
- Test hand-written concurrency with `loom` in the slow tier. Run `cargo +nightly miri test` there for any crate that contains `unsafe`.
- Rewrite or remove a flaky test only after preserving the behavior it protected.

## Release profile

```toml
[profile.release]
opt-level = 3
codegen-units = 1
lto = "fat"               # "thin" if link time hurts; usually within a few percent
strip = "symbols"
panic = "unwind"          # a panic stays inside its task; see errors.md

[profile.profiling]
inherits = "release"
debug = true
strip = "none"
```

- Use the `profiling` profile for flamegraphs and heap profiles so symbols resolve.
- Set the CPU target explicitly when the fleet is known: `x86-64-v3` or a specific arm64 core. Never `native` in CI.
- Try profile-guided optimization with `cargo-pgo` for a CPU-bound service once a representative workload exists.
- Release builds are slow by design. Never use them in the edit loop.

## CI

- Run the same `cargo t --workspace`, then `T_SLOW=1 cargo t --workspace` on the main branch or on a schedule.
- Cache `~/.cargo/registry`, `~/.cargo/git`, and `target/` keyed on the lockfile and toolchain. Cargo disables incremental compilation when `CI` is set; use `sccache` there if clean builds are slow.
- Fail on warnings with Cargo's `build.warnings = "deny"` setting, not by editing `RUSTFLAGS`.
- Run `cargo deny check` and `cargo machete`. Check `.sqlx/` freshness with `cargo sqlx prepare --workspace --check`.
- Keep `target/t-times.tsv` and `target/build-times.tsv` as build artifacts so a regression can be traced to a commit.
- Run benchmarks on a schedule against a saved baseline; do not gate every change on noisy timings.

## Keep backend builds independent

Backend checks depend only on backend sources, `Cargo.lock`, `.sqlx/`, and explicitly declared generated inputs. They must not install or build a client toolchain. When the binary embeds a built frontend, the backend build consumes the current staged directory; the full artifact pipeline owns freshness.
