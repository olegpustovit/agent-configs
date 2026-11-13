# Concrete types and minimal JSON transforms

Use this reference before changing API types, JSON, database rows, dynamic values, or layer transforms.

## Prefer one meaningful type

Use the entity directly as the response when its domain meaning and public JSON contract match. One struct with `Serialize` and `sqlx::FromRow` is better than three structs that copy the same six fields.

Add a type only when a boundary truly differs:

- **Write input:** what a caller may set, validated at the boundary. Never deserialize straight into an entity; that lets a client set `id`, `owner`, or `created_at`.
- **Response type:** only when the public shape differs materially from the entity (computed fields, a joined view, hidden internals).
- **Row type:** only when the stored shape differs from the entity.

## Export entities safely

- Apply `#[serde(rename_all = "camelCase")]` (or the API's convention) on the type, not field by field.
- Exclude internal fields statically with `#[serde(skip)]`. Never scrub a serialized `Value` afterwards.
- Put `#[serde(deny_unknown_fields)]` on write inputs so a typo is an error, not a silent no-op.
- Use `#[serde(default)]` deliberately; an absent field and an explicit `null` are different facts. Model the difference with `Option<Option<T>>` or a small enum when it matters, as in PATCH.
- Test exact serialized keys on sensitive boundaries with a snapshot (`insta`), so adding a field to an entity cannot silently publish it.

## Make states types

- Wrap identifiers in newtypes (`struct NoteId(String)`) with `#[serde(transparent)]` so a workspace ID cannot be passed where a note ID belongs.
- Model closed sets as enums with `#[serde(rename_all = "snake_case")]`, stored as text with a `CHECK` constraint.
- Use tagged enums (`#[serde(tag = "type")]`) for variants. Avoid `#[serde(untagged)]` on hot paths: it buffers the input and tries each variant in turn.
- Parse once at the boundary into a validated type and pass that type inward. Do not re-validate in every layer.

## Dynamic data

- Avoid `serde_json::Value` and `HashMap<String, Value>` in domain types. They allocate per node and push validation to every reader.
- When a payload must pass through untouched (a webhook body, an upstream tool result, stored `jsonb` returned as-is), keep it as `Box<serde_json::value::RawValue>` or `&RawValue`. It is validated JSON that is never parsed into a tree and is written back verbatim.
- When only a few fields of a large document matter, deserialize into a struct with just those fields; serde skips the rest without building it.

## Borrowing

Owned request types (`String`, `Vec<T>`) are the default and are correct for nearly every endpoint. Borrow only on a designated hot path, and then:

- Use `#[serde(borrow)] Cow<'a, str>`, not `&'a str`. A bare `&str` fails on any string containing an escape; `Cow` borrows when it can and owns when it must.
- Deserialize from the body bytes with `serde_json::from_slice` so borrowed fields point into the buffer.
- Serialize with `serde_json::to_writer` into a reused buffer instead of `to_string` followed by a copy.
- Keep the borrowed type private to the hot path. Lifetimes that spread into component signatures cost more in review and refactoring than they save.

See [allocation.md](allocation.md) for when this is worth doing and how to prove it.

## Other formats

- Stay on `serde_json` until a profile shows JSON above roughly a tenth of CPU time. Then evaluate `sonic-rs` or `simd-json` behind a benchmark on the deployment architecture; both carry substantial `unsafe`, and the speedup on arm64 is smaller than on x86-64.
- For internal binary data: `postcard` for a compact stable wire format, `rkyv` for zero-copy access to cached or memory-mapped data, `bitcode` for the smallest transient encoding (no format stability).
- Timestamps cross JSON as RFC 3339 strings in UTC. Durations cross as integer milliseconds or seconds with the unit in the field name.

## Map rows directly

Let `query_as!` build the entity. Do not introduce a row struct and a `From` impl that copy fields one to one. Map `jsonb` columns to a concrete type with `sqlx::types::Json<T>`, or to `RawValue` when the application only stores and returns them.
