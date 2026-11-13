# Architecture and dependency selection

Use this reference when scaffolding or restructuring crates and modules, selecting backend dependencies, or adding a dependency between components.

## Contents

- Workspace shape
- Binary composition
- HTTP transport
- Component module contract
- Dependency direction
- Baseline dependencies
- Language idioms that changed

## Workspace shape

Use a virtual workspace: the root `Cargo.toml` has no `[package]`, only members and shared settings.

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.100"

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal", "net", "time", "sync"] }
# every third-party version is declared here once

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "warn"
todo = "warn"
large_futures = "warn"
await_holding_lock = "deny"
redundant_clone = "warn"
needless_collect = "warn"
```

Each member sets `[lints] workspace = true` and takes dependencies as `tokio.workspace = true`. Allow a pedantic lint at the workspace level when it is noise for the codebase; do not scatter `#[allow]`.

Start with one `core` library crate. Split a component into its own crate when one of these is true: it has a second consumer, its compile time dominates the edit loop, or it needs `unsafe` (which then lives in a small crate that overrides the `forbid`). Cargo rejects cyclic crate dependencies, so a split also turns the dependency rule into a compile error.

## Binary composition

Keep `crates/api/src/main.rs` small enough to show the lifecycle:

1. Parse configuration from the environment and validate it.
2. Install tracing.
3. Build the Tokio runtime and enter `run`.
4. Open the pool and apply embedded migrations.
5. Construct repositories and component services.
6. Build the router and start background tasks.
7. Serve until the shutdown signal, then drain.

```rust
fn main() -> std::process::ExitCode {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => { eprintln!("configuration: {e:#}"); return std::process::ExitCode::from(2); }
    };
    logging::install(&config.log);
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build();
    match rt.map_err(anyhow::Error::from).and_then(|rt| rt.block_on(bootstrap::run(config))) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => { tracing::error!(error = format!("{e:#}"), "fatal"); std::process::ExitCode::FAILURE }
    }
}
```

Use the multi-thread runtime. Thread-per-core and io_uring runtimes do not pay off for a PostgreSQL-backed API; the database round-trip dominates.

## HTTP transport

Keep presentation in the binary crate:

```text
crates/api/src/
  server.rs        # listener, axum::serve, graceful shutdown
  routes.rs        # route table; no business logic
  handlers/
    health.rs
    auth.rs
    v1/*.rs        # thin handlers: extract, call component, map JSON
  middleware/auth.rs
  apijson.rs       # response mappers and the ApiError type
```

Rules:

- Application state is one cheap-to-clone struct of `Arc` handles, passed with `State`. Name each concrete service type once:

```rust
pub type Notes = note::Service<note::PgRepository>;

#[derive(Clone)]
pub struct AppState {
    pub notes: Arc<Notes>,
}
```

- Handlers extract typed inputs, resolve the principal from middleware, call one service method, and map the result. They return `Result<impl IntoResponse, ApiError>`.
- Use `tower-http` for timeouts, body limits, compression, and request tracing. Set a body limit and a timeout on every router.
- Components never import `axum`, `http`, or the `api` crate.

## Component module contract

```text
core/src/components/note/
  mod.rs          # public surface: re-exports only
  models.rs       # Note and related value types
  repository.rs   # Repository trait, PgRepository, and SQL
  service.rs      # use-case behavior
  error.rs        # the component's error enum
```

Keep items `pub(crate)` unless another crate needs them. The repository port uses return-position `impl Future` with an explicit `Send` bound so services stay usable from spawned tasks; implementors still write `async fn`:

```rust
pub trait Repository: Send + Sync + 'static {
    fn get(&self, id: &str) -> impl Future<Output = Result<Note, Error>> + Send;
    fn create(&self, input: NewNote) -> impl Future<Output = Result<Note, Error>> + Send;
}

pub struct Service<R> {
    repo: R,
}

impl<R: Repository> Service<R> {
    pub fn new(repo: R) -> Self { Self { repo } }

    pub async fn rename(&self, id: &str, title: &str) -> Result<Note, Error> {
        // validation and rules live here, not in the handler or the repository
        todo!()
    }
}
```

Tests construct `Service<FakeRepository>`. Nothing is boxed and nothing is dynamically dispatched.

## Dependency direction

- Draw the component graph as a DAG before adding an edge. A parent may call its children; siblings do not call each other.
- A workflow that spans components lives in the highest module that owns the use case, or in the binary's bootstrap if it is pure wiring.
- When a lower component needs behavior from a higher one, define a small trait in the lower component and implement it above. Invert only real boundaries.
- Shared infrastructure (`infrastructure/postgres`, `infrastructure/queue`) imports no component.
- Prefer passing data over passing services. A function that needs three values should take three values, not a service that can fetch them.

## Baseline dependencies

Versions current as of November 2026. Check crates.io before pinning; several of these release weekly.

| Concern | Default | Alternative and when |
| --- | --- | --- |
| Toolchain | Rust 1.100 stable, edition 2024 | none |
| Runtime | `tokio` 1.54, multi-thread | `LocalRuntime` per core only for a measured shared-nothing design |
| HTTP server | `axum` 0.8, `hyper` 1, `tower` 0.5, `tower-http` 0.7 | `actix-web` 4 when raw plaintext throughput is the product |
| HTTP client | `reqwest` 0.13 with rustls | `hyper-util` client for a minimal proxy hot path |
| PostgreSQL | `sqlx` 0.9 | `tokio-postgres` 0.7 with `deadpool-postgres` for a measured hot query |
| SQLite | `rusqlite` 0.42 inside `spawn_blocking` | `sqlx` sqlite only for API uniformity |
| JSON | `serde` 1, `serde_json` 1 | `sonic-rs` or `simd-json` only when a profile shows JSON dominating |
| Errors | `thiserror` 2 in components, `anyhow` 1 in binaries | `snafu` when context selectors are wanted throughout |
| Logging | `tracing` 0.1, `tracing-subscriber` 0.3 | OpenTelemetry crates when a collector exists; traces are still beta |
| CLI and env | `clap` 4 derive with `env` | hand-rolled `std::env` for tiny binaries |
| Secrets | `secrecy` 0.10 | none |
| Shutdown | `tokio-util` 0.7 `CancellationToken`, `TaskTracker` | none |
| Allocator | `mimalloc` | `tikv-jemallocator` for fragmentation-sensitive services and heap profiling |
| MCP | `rmcp` 3 | none |
| Embedded assets | `rust-embed` 8 | `memory-serve` for axum-native precompressed assets |
| Passkeys | `webauthn-rs` 0.6 | none |

Flag these as stale or unmaintained and do not add them: `envy`, `lasso`, `kanal`, `tokio-uring`, `glommio`, `assert_no_alloc`, `bytehound`, `oxide-auth` for new OAuth 2.1 work.

## Language idioms that changed

- `async fn` in traits and return-position `impl Trait` in traits are stable. Use them for static dispatch. `async fn` in `dyn Trait` is not stable; where dynamic dispatch is truly required, box that one future by hand.
- `std::sync::LazyLock` and `OnceLock` replace `once_cell` and `lazy_static`.
- Async closures, let chains (edition 2024), `if let` guards, and trait upcasting are stable. Use them instead of older workarounds.
- `std::sync::Mutex` is futex-based and fast. Use it by default; `parking_lot` 0.13 removed eventual fairness, so do not adopt it for fairness.
- The standard `Allocator` API stabilized in 1.100. Treat ecosystem support as new and verify a crate implements it before relying on `Vec<T, A>`.
- `gen` blocks and return-type notation are still unstable. Do not design around them.
