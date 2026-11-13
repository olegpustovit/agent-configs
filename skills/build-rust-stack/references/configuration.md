# Configuration, bootstrap, logging, and shutdown

Use this reference before adding configuration, secrets, tracing, startup, shutdown, or dependency construction.

## Configuration precedence

Apply typed defaults, then prefixed environment variables. Parse once into one struct, validate, and pass the struct (or the parts each constructor needs) explicitly. Nothing reads the environment after startup.

Use `clap` derive with the `env` attribute so one definition covers flags, environment, defaults, and `--help`:

```rust
#[derive(clap::Parser, Debug)]
pub struct Config {
    #[arg(long, env = "NOTES_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: std::net::SocketAddr,

    #[arg(long, env = "NOTES_DATABASE_URL")]
    pub database_url: secrecy::SecretString,

    #[arg(long, env = "NOTES_PUBLIC_URL")]
    pub public_url: url::Url,

    #[arg(long, env = "NOTES_LOG_FORMAT", default_value = "text")]
    pub log_format: LogFormat,
}
```

- Use one uppercase prefix per application. Document every variable in the README.
- Parse into real types: `SocketAddr`, `Url`, `Duration`, enums. A value that parses is a value later code can trust.
- Validate cross-field rules in one `validate(&self)` function and fail startup with every problem listed.
- When secrets come from Vault or another store, load them after the bootstrap variables, apply environment overrides last, and fail startup if a configured store cannot be reached.

## Keep secrets out of observable state

- Wrap secrets in `secrecy::SecretString`; it redacts `Debug` and zeroizes on drop. Call `expose_secret()` only at the point of use.
- Never derive `Debug` or `Serialize` on a struct that holds raw secret strings.
- Never log the full configuration. Log a short, explicit startup summary: listen address, public URL, log format, build SHA.

## Own composition in the binary

`bootstrap::run` constructs everything in order and returns when shutdown completes:

```rust
pub async fn run(config: Config) -> anyhow::Result<()> {
    let pool = postgres::open(&config).await.context("opening database")?;
    postgres::migrate(&pool).await.context("applying migrations")?;

    let state = AppState { notes: Arc::new(note::Service::new(note::PgRepository::new(pool.clone()))) };

    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    tasks.spawn(sweeper::run(pool.clone(), shutdown.clone()));

    let listener = tokio::net::TcpListener::bind(config.listen).await.context("binding listener")?;
    tokio::spawn(signal::cancel_on_signal(shutdown.clone()));

    axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(shutdown.clone().cancelled_owned())
        .await
        .context("serving")?;

    tasks.close();
    tokio::time::timeout(config.shutdown_grace, tasks.wait()).await.context("draining tasks")?;
    pool.close().await;
    Ok(())
}
```

- Dependency construction is plain code in one place. No container, no global registry.
- Share with `Arc` handles created here. Read-mostly values that change at runtime go in `arc_swap::ArcSwap`, not behind a lock.
- Every background task is spawned on the `TaskTracker` and selects on the `CancellationToken`.
- Handle both SIGTERM and SIGINT. The grace period is configuration and shorter than the service manager's stop timeout.

## Tracing

- Install one `tracing-subscriber` at startup: human-readable by default, JSON when configured, color off when stdout is not a terminal or `NO_COLOR` is set.
- Write to stdout and let the service manager collect it. Use the non-blocking writer if log volume is high.
- Add request spans with `tower-http`'s trace layer. Record method, matched route, status, and latency; never headers or bodies.
- Use `#[tracing::instrument(skip_all, fields(...))]` on service entry points. `skip_all` avoids formatting arguments and avoids leaking them.
- Log structured fields, not interpolated strings: `tracing::info!(note_id = %id, "note created")`.
- On hot paths, avoid `?value` (`Debug` formatting) and guard expensive fields with `tracing::enabled!`. A disabled event costs almost nothing; an enabled one formats and may allocate.
- Add OpenTelemetry export only when a collector exists. Its metrics and logs are stable; traces are still beta and the 0.x crates break on upgrade.

## Health

Expose `/healthz` (process is up) and a readiness check that verifies the pool. Report the build SHA and schema version so a deploy can confirm what is running.
