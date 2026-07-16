---
title: API reference
description: The generated Rust API, limits, return values, and runtime behavior shipped in v0.0.1.
order: 3
---

Build-time schema compilation emits typed rows, patches, selectors, keys, and changes. Applications pass those generated values to `db.get`, `db.write`, and `db.write_many`.

## Open a database

```rust
include!(concat!(env!("OUT_DIR"), "/sixpack_schema.rs"));
use sixpack_generated_schema as sdk;

let db = Database::open_local_with_schema(
    root,
    "assistant",
    sdk::database_schema(),
);
db.init()?;
```

The lightweight `sixpack::schema!` macro creates schema metadata only. Use `sixpack-schema-compiler` from `build.rs` when you need generated rows and selectors.

## Generated operations

| Operation | Behavior |
| --- | --- |
| `add(row)` | Insert once; reject duplicate ids and unique keys. |
| `set(row)` | Insert or replace one complete row. |
| `by::id(id)` | Read zero or one typed row. |
| `by::lookup(value)` | Read rows through a declared lookup. |
| `by::lookup(value).page(n)` | Read typed rows and a next cursor. |
| `all().limit(n)` | Scan typed rows and a next table cursor. |
| `count()` | Count current live rows. |
| `edit(key, patch)` | Patch one existing row through a unique key. |
| `remove(key)` | Append a tombstone for one existing row. |
| `write_many(&changes)` | Validate and append one same-table batch. |

## Read limits

- The default table page is 100 rows.
- An unpaged non-unique lookup returns at most 1,000 rows.
- Explicit page limits are from 1 through 1,000.
- Lookup rows are ordered lexicographically by row id.
- A cursor is a current-row offset, not a snapshot transaction.

## Write rules

- Add and set require every schema field.
- Patches cannot be empty or change the row id.
- One batch must stay within one table.
- Validation happens before the batch is appended.
- There are no cross-table or all-or-nothing transactions.
- There are no foreign keys or cascade deletes.

## Errors and async applications

`DatabaseError` distinguishes filesystem or format errors, schema errors, invalid plans, and not-found plans. Handle errors rather than assuming missing data and corrupted data are the same thing.

The API is synchronous. An async application should place database calls on a blocking worker. The supported shared-access profile is two low-traffic processes using the same local filesystem.

See the [complete API reference](https://github.com/memoirlabs/sixpack/blob/main/packages/docs/api.md) for generated type signatures, feature flags, cache maintenance, and explicitly unsupported behavior.
