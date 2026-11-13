# PostgreSQL persistence

Use this reference before adding or changing persistence. PostgreSQL is the default. Choose SQLite only when the application must be a single local file with no database service; choose one database when the app is scaffolded and do not support both in one application.

## Rules

1. **One connection setting:** `<APP>_DATABASE_URL`. Read it into typed configuration as a secret, validate it before startup, and never log it.
2. **Startup order:** load configuration, open the pool, apply embedded migrations, construct repositories and services, listen. If opening or migrating fails, the process exits without serving.
3. **Forward-only migrations:** keep SQL in `crates/core/migrations/NNNN_name.sql`, embed it with `sqlx::migrate!`, and run it at startup. Never edit an applied migration. Do not write down migrations or a migration command.
4. **Keep releases rollback-safe.** A release may add tables, nullable columns, and indexes. Defer drops and renames to a later release so the previous binary still runs after a binary rollback.
5. **Own SQL in the component.** Put the `Repository` trait and `PgRepository` in the component's `repository.rs`, with each statement written inline in the method that runs it.
6. **Check SQL at compile time.** Use `sqlx::query!` and `sqlx::query_as!` with positional parameters. Commit the `.sqlx/` metadata and build with `SQLX_OFFLINE=true` so compiling never needs a database.
7. **Never assemble SQL from input.** sqlx 0.9 only accepts `&'static str` or an explicit `AssertSqlSafe`; treat any use of `AssertSqlSafe` as a review flag.
8. **One repository method is one transaction.** Begin with `pool.begin()`, commit explicitly, and let drop roll back. A transaction stays inside one component.
9. **Translate errors in the repository.** Map missing rows to the component's `NotFound` and unique violations to its `Conflict`. No `sqlx::Error` crosses the component boundary.
10. **Column conventions:** application-generated text IDs, `timestamptz` timestamps, `jsonb` for structured JSON, text plus `CHECK` constraints instead of enum types, `Option<T>` for nullable columns, and keyset pagination.
11. **Search:** a stored generated `tsvector` column with a GIN index, `websearch_to_tsquery`, and `ts_headline`.
12. **Expiry:** PostgreSQL has no TTL indexes. One tracked background task deletes expired rows on an interval and stops on the shutdown token.
13. **Repository tests run against real PostgreSQL.** Use `#[sqlx::test]`, which gives each test a fresh migrated database.

## Repository shape

```rust
pub struct PgRepository {
    pool: sqlx::PgPool,
}

impl PgRepository {
    pub fn new(pool: sqlx::PgPool) -> Self { Self { pool } }
}

impl Repository for PgRepository {
    async fn get(&self, id: &str) -> Result<Note, Error> {
        sqlx::query_as!(
            Note,
            "SELECT id, title, body, version, created_at, updated_at
               FROM notes
              WHERE id = $1",
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .ok_or(Error::NotFound)
    }
}

fn map_db(e: sqlx::Error) -> Error {
    match e {
        sqlx::Error::RowNotFound => Error::NotFound,
        sqlx::Error::Database(ref d) if d.is_unique_violation() => Error::Conflict,
        other => Error::Storage(Box::new(other)),
    }
}
```

`PgPool` is already a cheap reference-counted handle; clone it, do not wrap it in another `Arc`.

Verify macro and method names against docs.rs for the pinned sqlx version before writing new patterns. The crate moved to `github.com/transact-rs/sqlx` and 0.9 changed feature flags and the query argument type.

## Pool and startup

- Size the pool from configuration. A good start is a small multiple of the CPU count, well under the server's `max_connections`.
- Set an acquire timeout and a statement timeout. A request must fail fast when the pool is exhausted.
- Run `sqlx::migrate!("./migrations").run(&pool)` once at startup. Confirm in the pinned version's docs that the PostgreSQL migrator takes its advisory lock, so two instances starting together do not race.
- Expose the applied schema version in the health response.

## Query performance

Do these before any micro-optimization; they are worth more than every allocation trick combined.

- Count round-trips per request. Collapse N+1 loops into one query with `= ANY($1)`, a join, or `UNNEST` for batch writes.
- Select only the columns the method returns.
- Stream large result sets with `fetch` and process rows as they arrive; do not `fetch_all` unbounded results.
- Use keyset pagination, never `OFFSET` for deep pages.
- Read the plan (`EXPLAIN (ANALYZE, BUFFERS)`) for any query on a hot path and add the index it needs.
- Keep transactions short and never await an external call while holding one.

`sqlx` row mapping produces owned `String`s. That is fine for almost every endpoint. When a profile shows one query's mapping or protocol overhead dominating, move that single repository method to `tokio-postgres` with `deadpool-postgres`: it pipelines automatically, caches prepared statements with `prepare_cached`, and lets a row lend `&str`. Keep the change inside the repository. Record the measurement that justified it; no current public benchmark settles the gap.

## Tests

```rust
#[sqlx::test(migrations = "./migrations")]
async fn create_then_get_round_trips(pool: sqlx::PgPool) {
    let repo = PgRepository::new(pool);
    let created = repo.create(NewNote { title: "a".into(), body: String::new() }).await.unwrap();
    let loaded = repo.get(&created.id).await.unwrap();
    assert_eq!(created, loaded);
}
```

- Write one contract test module per repository: round trip, not found, conflict, pagination order, and concurrent update behavior.
- Test migrations separately against an empty database, including two concurrent starters.
- Keep service tests free of the database by using a fake repository.
- After changing any query, run `cargo sqlx prepare --workspace` and commit `.sqlx/`. CI runs `cargo sqlx prepare --workspace --check`.

## SQLite for single-file applications

Use `rusqlite` with the `bundled` feature. It is synchronous: own the connection on a dedicated thread or call it inside `spawn_blocking`, never directly on a runtime worker. Enable WAL and a busy timeout, keep one writer, embed migrations as ordered SQL files applied at startup, and use FTS5 with triggers for search.
