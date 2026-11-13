# Errors, panics, and final-boundary logging

Use this reference before changing error handling, retries, HTTP responses, worker failures, or logging.

## Two layers of error type

- **Components** define one `thiserror` enum each. Variants are the outcomes a caller can act on, plus one opaque variant for everything else.
- **Binaries** use `anyhow::Result` for startup, wiring, and task supervision, adding context with `.context("opening pool")`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("note not found")]
    NotFound,
    #[error("note already exists")]
    Conflict,
    #[error("invalid note: {0}")]
    Invalid(&'static str),
    #[error("storage failure")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}
```

Rules:

- Variants name domain outcomes, not the library that failed. No `Sqlx(sqlx::Error)` or `Reqwest(reqwest::Error)` in a public enum; that leaks the driver across the boundary and into every caller's `match`.
- Keep the source chain with `#[source]` so the final log shows the cause.
- Do not derive `From` for foreign errors by habit. A blanket `From<sqlx::Error>` lets `?` skip the translation that turns a unique violation into `Conflict`.
- Do not use `anyhow` in component signatures. Callers cannot match on it.
- Error messages are lowercase, have no trailing punctuation, and do not repeat their source's text.

## Add context where it is known

The layer that knows which operation and which identifier failed adds that context. In binaries use `.with_context(|| format!("loading workspace {id}"))`; the closure runs only on failure, so the happy path does not format or allocate.

## Return upward, log once

Lower layers return errors. The final operational boundary logs exactly once:

- the HTTP error mapper, for a request;
- the task supervisor, for a background task;
- `main`, for startup and shutdown.

Log the full chain there with `{e:#}` for `anyhow` or by walking `source()`. A function that logs and also returns the error produces duplicate, context-free lines.

## Map errors at the HTTP edge

One `ApiError` type in the binary implements `IntoResponse`. It maps each component variant to a status and a stable machine-readable code, logs server-side failures, and never sends internal detail to the client.

```rust
impl From<note::Error> for ApiError {
    fn from(e: note::Error) -> Self {
        match e {
            note::Error::NotFound => ApiError::not_found("note_not_found"),
            note::Error::Conflict => ApiError::conflict("note_exists"),
            note::Error::Invalid(why) => ApiError::bad_request("invalid_note", why),
            other => ApiError::internal(other),   // logged here, generic body
        }
    }
}
```

## Panics

- A panic is a bug, not an error path. Do not `unwrap`, `expect`, index out of bounds, or `panic!` on request paths.
- `expect` is acceptable for startup invariants and for conditions the code just established; write the invariant in the message.
- Keep `panic = "unwind"` for servers. Tokio then contains a panic to the task that raised it, and the other in-flight requests finish.
- Supervise background tasks: await their `JoinHandle`, treat a panic or unexpected exit as an error, log it once, and restart or shut down deliberately. A task that dies silently is the worst outcome.

## Cancellation, timeouts, retries

- Every `.await` is a cancellation point. A future dropped mid-way must not leave shared state half-updated; finish critical sections before awaiting, or move them into a spawned task that runs to completion.
- Put a timeout on every outbound call and every pool acquire. Translate elapsed timeouts into a distinct variant.
- Retry only idempotent operations, with bounded attempts, backoff, and jitter. Never retry inside a transaction.
- Preserve cancellation identity: a shutdown-driven cancellation is not a failure and should not be logged as one.

## Test error behavior

Assert on variants with `matches!`, not on message strings. Test that each repository translates not-found and conflict correctly, and that the HTTP mapper returns the documented status and code for each variant without leaking the source.
