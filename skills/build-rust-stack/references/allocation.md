# Low-allocation hot paths

Use this reference before optimizing a hot path, choosing types for performance, changing the global allocator, or claiming code is allocation-free.

## Contents

- Decide whether to optimize at all
- Free wins
- Async runtime rules
- Hot-path rules
- Type choices
- Global allocator
- Measure and enforce

## Decide whether to optimize at all

A PostgreSQL round-trip costs roughly 0.2 to 2 ms. A heap allocation costs roughly 20 to 50 ns. A handler that makes fifty allocations around one query spends under one percent of its time allocating. For a network-bound endpoint, the order of work is:

1. Fewer round-trips: batch, join, pipeline, cache.
2. Better queries: indexes, narrower selects, keyset pagination.
3. Smaller payloads and fewer bytes serialized.
4. Correct pool sizes, timeouts, and concurrency limits.
5. Only then, CPU and allocation work, and only where a profile points.

Zero-allocation code is the right goal for a small, named set of paths: parsers and encoders, protocol framing, proxies and pass-through, fan-out loops, in-memory indexes and caches, and anything called per item rather than per request. Write those deliberately. Write everything else as clear, owned, boring Rust.

Lifetime-heavy types and `unsafe` have a real cost in review time, refactoring friction, and bug surface. Do not pay it on a path nobody measured.

## Free wins

Take these before touching code:

- The release profile in [performance.md](performance.md): `opt-level = 3`, `codegen-units = 1`, LTO.
- A modern global allocator (below).
- A sized connection pool and HTTP client reuse. Construct one `reqwest::Client` and clone it.
- Compression and body limits at the edge.
- Profile-guided optimization for a CPU-bound service: typically 5 to 15 percent.

## Async runtime rules

These matter more than allocation counts for tail latency:

- Never block a runtime worker. File I/O, password hashing, compression of large bodies, and synchronous libraries go through `spawn_blocking` or a dedicated thread pool. Aim for well under a millisecond between `.await` points.
- Never hold a `std::sync::Mutex` guard, `RwLock` guard, or `DashMap` reference across `.await`. Copy out what you need and drop the guard first. `clippy::await_holding_lock` is set to deny.
- Bound everything: channel capacity, in-flight requests (`tower` concurrency limit), fan-out (`Semaphore` or `buffer_unordered(n)`), pool size, retry attempts.
- Keep futures small. A large array or buffer held across `.await` is copied into the task; box the cold branch or move the buffer to the heap once. `clippy::large_futures` catches this.
- Prefer `tokio::join!` and `FuturesUnordered` in the current task over spawning when the work is I/O-bound and short; a spawn allocates a task.

## Hot-path rules

Each rule is checkable in review.

- **No `format!`, `to_string`, or `String` concatenation.** Write into a reused buffer with `write!`; use `itoa` and `ryu` for numbers.
- **No `clone()` of `String`, `Vec`, or large structs.** Borrow, move, or share with `Arc<str>` / `Arc<[T]>` / `bytes::Bytes`, whose clones are reference-count increments.
- **No intermediate `collect()`.** Return `impl Iterator` and consume lazily. Collect once, at the end, into a pre-sized container.
- **Pre-size and reuse.** `Vec::with_capacity`, then `clear()` and reuse across iterations. A cleared `Vec` keeps its capacity.
- **Pass slices, not owned containers.** Take `&str`, `&[T]`, and `impl AsRef<[u8]>` in signatures. Take ownership only when the function stores the value.
- **Use `Cow<'_, str>` for maybe-modified data** so the common unmodified case does not copy.
- **Static dispatch.** Generics or enums on the hot path; `dyn Trait` at composition seams where a call is made once per request, not once per item.
- **No boxed futures.** Use `async fn` in traits with static dispatch. Do not add `async-trait` or `BoxFuture` to a path called per item.
- **Bytes in, bytes out.** Read bodies as `Bytes`, slice without copying, and serialize with `to_writer` into a reused `BytesMut`.
- **Hash once.** Use the `entry` API instead of `contains_key` followed by `insert`.
- **Add `#[inline]` only with benchmark evidence,** and usually only on small cross-crate functions.

## Type choices

Adopt a crate from this table only when a profile shows the standard type is the cost.

| Need | Default | Alternative and when |
| --- | --- | --- |
| Mostly-small vectors | `smallvec` 1 | `arrayvec` when capacity is fixed and heap is never wanted; `tinyvec` to avoid `unsafe` |
| Mostly-short strings | `compact_str` | `smol_str` for immutable strings cloned often; `ecow` for copy-on-write |
| Shared immutable text | `Arc<str>` | `bytes::Bytes` when the data is a slice of a larger buffer |
| Per-request scratch memory | `bumpalo` arena, reset per request or batch | the standard `Allocator` API once the crate in use supports it |
| Hash maps | `std::collections::HashMap` | `hashbrown` with `foldhash` when hashing shows in a profile and keys are not attacker-controlled; `rustc-hash` for integer keys |
| Concurrent maps | `papaya` for read-heavy use; it has no guard to hold across `.await` | `dashmap` for write-heavy use; `scc` |
| Caches | `moka` when TTL and async loading are needed | `quick_cache` for a lighter, faster bounded cache |
| Read-mostly shared state | `arc-swap` | `tokio::sync::watch` when readers must be notified |
| Locks | `std::sync::Mutex` | `parking_lot` for its API extras, not for speed or fairness |
| Channels | `tokio::sync::mpsc`, bounded | `flume` to bridge sync and async; `crossbeam-channel` between threads |
| Interning | a small `HashSet<Arc<str>>` | none maintained |

Keep an attack-resistant hasher (the standard one) for any map keyed by client-supplied data.

## Global allocator

Set one in each binary crate:

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

- `mimalloc` is the default. The crate now builds mimalloc v3. One production report showed resident memory growing without bound under v3 where v2 was stable, so soak-test RSS for a long-running service and enable the crate's `v2` feature if it climbs.
- `tikv-jemallocator` is the alternative when fragmentation over days matters or heap profiling is wanted. jemalloc is maintained again after a 2025 archive. On arm64, its page size is fixed at build time; a kernel with 16K or 64K pages needs it configured to match.
- Always set an allocator on musl targets. musl's built-in allocator is very slow under threads.
- Changing the allocator is a measured change: compare throughput, tail latency, and RSS after hours, not seconds.

## Measure and enforce

No performance claim without one of these.

- **Benchmarks:** `divan` or `criterion` in `benches/`. `divan`'s `AllocProfiler` reports allocation counts and bytes beside timings. Benchmark with realistic inputs and compare against a saved baseline.
- **CPU profiles:** `samply` or `cargo flamegraph` on a release build with debug info, under representative load.
- **Heap profiles:** `dhat` for where allocations come from; `heaptrack` or jemalloc profiling for a running service.
- **Async behavior:** `tokio-console` for tasks that never yield or wake too often.

Lock in a hot path with a test that fails when someone adds an allocation:

```rust
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn encode_frame_does_not_allocate() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let mut out = Vec::with_capacity(4096);
    let frame = sample_frame();

    let before = dhat::HeapStats::get();
    encode_frame(&frame, &mut out);
    let after = dhat::HeapStats::get();

    dhat::assert_eq!(after.total_blocks, before.total_blocks);
}
```

Put such tests in their own integration-test file: `dhat` testing mode needs to be the only allocator user in the process, and the test must run single-threaded. `allocation-counter` is a smaller alternative for the same assertion.

State the budget in a comment on the function ("zero allocations after warm-up", "one allocation per message") so the test and the intent stay together. When a budget must rise, change the test in the same commit and say why.
