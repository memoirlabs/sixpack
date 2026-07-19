# Public API Reference

This page describes the shipped `v0.0.1` Rust API. For a complete application
flow, see [AI Chat and Notes](ai-chat-notes.md).

## Recommended layer

Normal applications should use the build-time generated SDK:

```txt
schema.sixpack
  -> sixpack-schema-compiler in build.rs
  -> typed Row / Patch / by / key / changes
  -> Database::get / write / write_many
```

The generated code builds validated internal plans. `PlanEnvelope`, raw
`Record`, string-based selectors, and `LocalStore` are public for compatibility
and advanced use, but they expose more opportunities for application mistakes.

The `sixpack::schema!` macro emits runtime `TableSchema`/`DatabaseSchema`
metadata only. Typed rows and selectors come from
`sixpack_schema_compiler::emit_raw_rust`.

## Database lifecycle

```rust
let db = sixpack::Database::open_local_with_schema(
    root,
    workspace_name,
    generated::database_schema(),
);
db.init()?;
```

- `root` is the parent directory.
- `workspace_name` becomes the child database directory.
- `init()` creates missing layout, validates existing chunks, rebuilds caches,
  writes recoverable metadata, and is suitable for normal startup.
- Every process opening a workspace must use the same logical schema.
- `Database` is cloneable, but independently opened handles are also supported.

## Generated table API

For a generated table module named `messages`:

### Rows

```rust
messages::Row { /* every schema field */ }
```

Every field is required and uses one of:

| Schema type | Rust type |
| --- | --- |
| `id` | `String` |
| `text` | `String` |
| `int` | `i64` |
| `float` | `f64` |
| `bool` | `bool` |

### Create and replace

```rust
db.write(messages::add(row))?; // insert only
db.write(messages::set(row))?; // insert or complete replacement
```

- `add` fails with `AlreadyExists` when the id already exists.
- Both operations require a complete schema-valid row.
- Unique lookup conflicts fail before append.

### Read one

Every table has an implicit unique id selector:

```rust
let row: Option<messages::Row> = db.get(messages::by::id(id))?;
```

A schema declaration such as `lookup external_id unique` also generates a
selector returning `Option<Row>`.

### Read a non-unique lookup

```rust
let rows: Vec<messages::Row> =
    db.get(messages::by::conversation_id(conversation_id))?;
```

This convenience selector requests up to 1,000 rows. Use paging when a lookup
can exceed that size.

### Page a lookup

```rust
let (rows, next_cursor): (Vec<messages::Row>, Option<String>) = db.get(
    messages::by::conversation_id(conversation_id).page(100),
)?;

let (more, next_cursor) = db.get(
    messages::by::conversation_id(conversation_id)
        .page(100)
        .cursor(next_cursor.unwrap()),
)?;
```

Limits must be between 1 and 1,000. Cursors are opaque offsets over current
live rows, not snapshots. Lookup results are ordered by row id.

### Scan a table

```rust
let (rows, next_cursor) = db.get(messages::all().limit(100))?;
let (more, next_cursor) = db.get(
    messages::all().limit(100).cursor(next_cursor.unwrap()),
)?;
```

Without `.limit(...)`, scans use a default limit of 100.

### Count

```rust
let count: usize = db.get(messages::count())?;
```

The generated API currently exposes a whole-table count. The lower-level
`GetCount::lookup(...)` can count a declared lookup key when needed.

### Patch

```rust
db.write(messages::edit(
    messages::key::id(id),
    messages::Patch::new()
        .body("updated")
        .status("completed"),
))?;
```

- A patch must contain at least one field.
- `id` cannot be changed.
- Only schema fields are accepted.
- Fields omitted from the patch are preserved.
- The target must use id or another unique lookup and must exist.
- Internally the result is appended as a complete replacement row.

### Remove

```rust
db.write(messages::remove(messages::key::id(id)))?;
```

The target must exist and use a unique key. Removal appends a tombstone.

### Same-table batch

```rust
let results = db.write_many(&[
    messages::edit(messages::key::id(first), first_patch),
    messages::edit(messages::key::id(second), second_patch),
])?;
```

- Empty batches return an empty result.
- Every change must target one table.
- Insert-only batches are supported.
- Inserts cannot be mixed with set/edit/remove operations.
- A row id cannot appear twice.
- Validation completes before append.
- The batch uses one append/sync, but is not an all-or-nothing transaction after
  an underlying partial filesystem write.
- Cross-table transactions are not available.

## Generated table handles

Import the generated `GeneratedTables` trait to use table handles:

```rust
use generated::GeneratedTables;

db.messages().insert(row)?;
db.messages().patch(messages::key::id(id), patch)?;
let row = db.messages().get().id(id)?;
let rows = db.messages().find().conversation_id(conversation_id)?;
let (rows, cursor) = db.messages().scan().limit(100).run()?;
let count = db.messages().count()?;
```

These are equivalent generated conveniences. `find()` returns up to 1,000 rows
and does not expose a next cursor; use `db.get(table::by::lookup(...).page(n))`
for paged lookup reads.

## Errors

```rust
pub enum DatabaseError {
    Io(std::io::Error),
    Schema(SchemaError),
    Plan(PlanError),
}

pub enum PlanError {
    Invalid(String),
    NotFound(String),
}
```

- `Io` includes filesystem, format, corruption, locking, and duplicate insert
  errors. Duplicate inserts use `std::io::ErrorKind::AlreadyExists`.
- `Schema` reports invalid rows, fields, types, names, and lookup declarations.
- `Plan::Invalid` reports an unsupported or malformed request.
- `Plan::NotFound` reports a missing patch/remove target.

Errors are returned to the caller. The runtime does not silently delete
canonical data to hide corruption.

## TypeScript API

Generate a typed module from the same schema used by Rust:

```sh
sixpack generate typescript schema.sixpack > sixpack-schema.ts
```

The `@sixpack/db` package exposes asynchronous `db.get(...)`, `db.write(...)`,
and `db.writeMany(...)` calls over generated selectors and changes. Schema
`int` fields are exact TypeScript `bigint` values matching Rust `i64`. The
`int64(...)` helper converts ordinary safe numbers such as `Date.now()` or
decimal strings while checking the signed 64-bit range. The current
implementation starts a short-lived Rust bridge process for each operation; it
does not reimplement `.6` storage in TypeScript.

Generated rows are readonly snapshots. Generated types require complete rows
for add/set, reject empty patches, keep unique keys attached to their table,
and reject mixed-table `writeMany` calls at compile time. Runtime validation
still enforces the same boundaries for untyped JavaScript callers.

The TypeScript API currently supports unique and non-unique lookups, lookup
and table pages, counts, add, set, edit, remove, and same-table batches. It does
not implement `watch`.

## Maintenance API

```rust
db.rebuild_cache("messages")?;
```

`.6b` is derived state and can be rebuilt from canonical `.6` rows.

Compaction is excluded from default builds. It requires:

```toml
sixpack = { /* source */, features = ["experimental-compaction"] }
```

and exposes `db.compact_table(...)`. It is experimental maintenance—not needed
for normal reads and writes and not yet crash-hardened enough for the default
application contract.

## Concurrency and execution

- The supported profile is two low-traffic processes on one local filesystem.
- Reads take a shared workspace lock.
- Writes, initialization, cache rebuilds, and compaction take an exclusive
  workspace lock.
- Canonical appends are synced before a clean revision is published.
- Revision changes invalidate stale in-process caches.
- The API is synchronous. Use a blocking thread/worker from async runtimes.
- Multi-host and network-filesystem access are unsupported.

## Not implemented

- `db.watch(...)` subscriptions
- SQL or generic query strings
- ad hoc predicates/sorts/joins
- cross-table transactions
- foreign keys/cascade deletes
- optional/list/JSON/blob values
- full-text `.6x` search
- repair/inspect CLI commands
- stable generated-code snapshot compatibility across future pre-1.0 releases
