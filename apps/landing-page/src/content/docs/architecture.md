---
title: Architecture
description: How typed calls become readable local files and rebuildable lookup state.
order: 4
---

sixpack is table-first. It does not emulate a SQL page engine. Canonical rows remain inspectable, while generated binary projections accelerate the paths used by normal application code.

## Write path

```text
validate change
take workspace write lock
publish dirty revision
append and sync .6 row operation
update runtime projection
publish clean revision
```

An exclusive workspace lock serializes canonical mutations. Publishing a dirty revision before the append gives the next opener enough information to distinguish an interrupted tail from unexpected clean-state corruption.

## Read path

```text
typed selector
  -> generated row pointer or lookup entries
  -> canonical .6 segment slice
  -> decoded typed row
```

Point reads and declared lookups resolve through generated state first. Scans remain available, but ordinary application access should use ids and schema-declared lookup fields.

## Runtime projection

| Map | Purpose |
| --- | --- |
| `rows_by_id` | row id to `.6` row pointer |
| `lookup_ids` | lookup field and key to live row ids |
| `row_lookup_keys` | row id to the lookup keys currently attached to it |

The reverse lookup map lets an edit detach old keys without traversing the entire table.

## Package boundaries

| Package | Responsibility |
| --- | --- |
| `sixpack-core` | Shared schema, row, field, lookup, and value types. |
| `sixpack-format` | Durable `.6` and generated cache encoding boundaries. |
| `sixpack-store` | Local directory-backed storage engine. |
| `sixpack` | Composed public database API. |
| `sixpack-cli` | Binary command parsing and execution. |
| `sixpack-schema-compiler` | Schema parsing, validation, and generated Rust output. |

## Concurrency and recovery

The supported profile is two low-traffic processes on one local filesystem. Reads use shared workspace locks; canonical writes use an exclusive lock. Revision changes invalidate stale row, chunk, and generated caches in independently opened handles.

Canonical `.6` files are authoritative. `sixpack.toml` counters and generated `.6b` indexes can be checked and rebuilt from the schema and canonical table data.
