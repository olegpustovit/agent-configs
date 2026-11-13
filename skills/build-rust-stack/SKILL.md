---
name: build-rust-stack
description: Structure, implement, refactor, or review Rust backend services using a Cargo workspace, Tokio and axum, thin binary crates that own composition and HTTP transport, component-owned domain and PostgreSQL persistence with compile-time-checked hand-written SQL (sqlx) and embedded forward-only migrations, thiserror component errors with anyhow context and final-boundary logging, static dispatch over needless trait objects, prefixed environment configuration, the official rmcp MCP SDK, measured low-allocation hot paths, and a fast edit loop: a `cargo t` wrapper that kills any test case over a hard deadline (2 s unit, 5 s integration), measured incremental build budgets, and a tuned dev profile. Use for Rust workspace layout, crates and modules, components, repositories, services, JSON/API contracts, handlers, workers, CLIs, configuration, secrets, tracing, startup, shutdown, allocation and performance work, allocators, release profiles, slow or hanging tests, test timeouts, slow builds, incremental compilation, deployment, or architecture reviews. This skill is backend-only.
---

# Build Rust Stack

Build an idiomatic Rust backend as a Cargo workspace: thin binary crates over one core library of components. Keep repository instructions and the requested product architecture authoritative.

## Start with discovery

1. Inspect repository instructions, `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, crate layout, and established conventions before editing.
2. Preserve compatible choices. Do not restructure unrelated code merely to match this skill.
3. Read [references/architecture.md](references/architecture.md) before scaffolding, moving crates or modules, selecting dependencies, or adding a component dependency.
4. Read [references/persistence.md](references/persistence.md) before adding or changing persistence, repositories, migrations, or database tests.
5. Read [references/serialization.md](references/serialization.md) before changing API types, JSON, database rows, dynamic values, or layer transforms.
6. Read [references/errors.md](references/errors.md) before changing error handling, retries, HTTP responses, worker failures, panics, or logging.
7. Read [references/configuration.md](references/configuration.md) before adding configuration, environment variables, secrets, tracing, startup, shutdown, or dependency construction.
8. Read [references/allocation.md](references/allocation.md) before optimizing a hot path, choosing collection or string types for performance, changing the global allocator, or claiming a path is allocation-free.
9. Read [references/performance.md](references/performance.md) before running or writing tests, and before changing build profiles, linkers, crate boundaries, CI caches, or developer feedback loops. It defines the `cargo t` wrapper, the test deadlines, and the build-time budgets.
10. Read [references/deploy.md](references/deploy.md) before changing release, cross-compilation, deployment, service-manager configuration, or database backups.
11. Read [references/mcp.md](references/mcp.md) before adding or auditing an MCP command, endpoint, transport, tool, or client.
12. Confirm unclear behavior only when a reasonable assumption could materially change the result.

## Use the standard workspace layout

Use this target unless the repository has a compatible variation:

```text
backend/
  Cargo.toml                # virtual workspace: members, [workspace.dependencies], [workspace.lints], profiles
  rust-toolchain.toml       # pinned stable toolchain
  .cargo/config.toml        # linker and target settings
  .sqlx/                    # committed offline query metadata
  tools/
    t/                      # the `cargo t` test runner, copied from assets/t
  crates/
    api/                    # binary: composition and HTTP transport
      src/
        main.rs
        config.rs
        bootstrap.rs
        server.rs
        routes.rs
        handlers/
        middleware/
        apijson.rs
    worker/                 # binary, only if needed
    core/                   # library: business rules and adapters
      migrations/
        0001_init.sql
      src/
        components/
          workspace/
          project/
          issue/
          auth/
        infrastructure/
          postgres/
          queue/
```

Add only crates the application needs. Keep axum handlers, extractors, middleware, route tables, and JSON response mappers in the binary crate. Keep component rules and adapters in `core`. Use **component** consistently for business modules.

## Compose and observe explicitly

- Use one uppercase application prefix for application environment variables. Parse them into one typed config struct, validate once, then construct dependencies.
- Keep `main` minimal: build the runtime, call an async `run(config) -> anyhow::Result<()>`, and map the result to an exit code.
- Construct pools, repositories, services, router, background tasks, and shutdown explicitly in `bootstrap.rs`. No global state, no service locator, no `lazy_static` singletons for dependencies.
- Use `tracing` for logs. Log a failure once at its final operational boundary; lower layers add context and return it.
- Default to readable stdout logs, honor `NO_COLOR`, and support JSON through configuration.
- Shut down with one `CancellationToken` and one `TaskTracker`. Every spawned task is tracked and observes the token.

## Preserve component ownership

- Treat humans and agents as first-class actors wherever the domain permits ownership, responsibility, authorship, participation, or assigned work.
- Keep each bounded context under `core/src/components/<name>/`; do not create a shared models dump.
- Keep the service-facing `Repository` trait and the concrete `PgRepository` together in the component's `repository.rs`, with SQL beside the methods that run it.
- Make services generic over their repository (`Service<R: Repository>`). The binary names the concrete type once. This is static dispatch: no boxed futures and no vtable on the request path.
- Add a trait only for a real replacement boundary, an important fast fake, or multiple implementations. Do not put a trait in front of every struct.
- Keep the module graph acyclic even though the compiler does not enforce it inside a crate. A component may depend on components below it, never sideways in a cycle.
- Keep HTTP types out of components and `sqlx` types out of handlers and service signatures.

## Keep data flow concrete

- Use an entity directly when its domain semantics and public JSON contract match.
- Add write inputs for authority and validation, and response types only for materially different public contracts.
- Make invalid states unrepresentable with enums and newtypes; parse at the boundary, then pass typed values.
- Avoid `serde_json::Value`, stringly-typed maps, and entity-row-DTO chains that copy the same fields. Decode unavoidable dynamic data immediately, or pass it through untouched as `RawValue`.
- Exclude internal fields statically and test exact serialized keys on sensitive boundaries.

## Spend performance effort where it pays

- Take the free wins first: release profile, global allocator, connection pooling, fewer database round-trips.
- Measure before changing code. A claim about speed or allocations needs a benchmark, a profile, or an allocation-count test.
- Designate hot paths explicitly (parsers, encoders, fan-out, caches, proxies). Give each an allocation budget and a test that enforces it.
- Everywhere else, write clear owned code. A PostgreSQL round-trip costs four orders of magnitude more than an allocation.
- Never block the runtime, never hold a lock across `.await`, and bound every queue, pool, and concurrent fan-out.

## Select dependencies deliberately

1. Search the standard library first, then the Tokio/Tower ecosystem, then established crates.
2. Verify current maintenance, last release, license, advisories, `unsafe` surface, feature flags, and MSRV before adding a dependency. Check crates.io and docs.rs; never guess an API or a version from memory.
3. Declare every version once in `[workspace.dependencies]` and disable default features you do not use.
4. Prefer mature crates for protocols, validation, cryptography, retry, and observability; avoid speculative wrappers.
5. Run `cargo deny check` for advisories, licenses, and duplicate versions.
6. Use the official `rmcp` crate for MCP lifecycle, schemas, stdio, and Streamable HTTP. Keep tool handlers thin over component services.

## Iterate fast, under hard deadlines

Fast builds and fast tests are requirements with numbers, defined in [references/performance.md](references/performance.md).

- Run tests only through `cargo t`. If the workspace lacks it, install it from [assets/t](assets/t) before writing tests. Never run raw `cargo test` or a test executable.
- `cargo t` kills any case that exceeds its deadline: 2 s for unit tests, 5 s for integration tests, 60 s for the whole run. `T_SLOW=1` runs the `#[ignore]` slow tier at 60 s per case and 600 s per run.
- Never raise a deadline, add a retry, or weaken an assertion to get a pass. Shrink the fixture, share setup, remove real sleeps and network, or move a truly exhaustive case to the slow tier.
- Inner loop: `cargo check -p <crate>`, then `cargo t -p <crate> <filter>`. A focused warm run finishes in under 10 seconds. Run touched crates and their consumers at a stable boundary; run the workspace once before handoff; run the slow tier only for final qualification.
- Before handoff also run `cargo fmt --check` and `cargo clippy --workspace --all-targets`.
- Keep debug builds incremental: one warm shared `target/`, no routine `cargo clean`, no per-command `RUSTFLAGS` or profile overrides, the dev profile from the reference.
- Measure the rebuild after an edit with `cargo t --touch <file> --budget <seconds> -p <binary>` when adding a dependency, a feature, a procedural macro, or a crate boundary. Fix an exceeded budget before continuing.
- Add focused service tests with small fakes and repository contract tests against real PostgreSQL.
- Report commands, wall times, the slowest cases, failures, assumptions, and skipped checks.
- Keep client implementation, visual design, and client toolchains outside this backend skill.

## Reject recurring failure modes

- Do not put business logic in handlers, `main.rs`, macros, or database adapters.
- Do not `unwrap`, `expect`, or `panic!` on request paths. Reserve them for startup invariants and tests.
- Do not add an ORM or query builder; keep SQL visible in the owning component. Never build SQL with `format!`.
- Do not edit an applied migration, add a down migration, or add a manual migration command. Embed forward-only migrations and apply them during startup.
- Do not span a transaction across components.
- Do not reach for `Arc<Mutex<T>>` as a default. Prefer ownership, message passing, or read-mostly snapshots.
- Do not use `async-trait`, `once_cell`, or `lazy_static` in new code; the language and standard library cover them.
- Do not box futures or use `dyn Trait` on a hot path without a measured reason, and do not add lifetimes to DTOs on cold paths to save an allocation nobody measured.
- Do not write `unsafe` outside a designated, Miri-tested crate.
- Do not log and return the same error, or expose a driver error type across a component boundary.
- Do not call blocking I/O, heavy CPU work, or `std::thread::sleep` on a runtime worker.
- Do not change the allocator, hasher, JSON library, or runtime flavor without a before-and-after measurement.
- Do not run raw `cargo test`, raise a test deadline, or leave a hanging or slow test in the fast tiers.
- Do not use real sleeps or real network in tests.
- Do not rebuild unrelated toolchains, clear caches, override profiles per command, or run the workspace suite on every edit.
