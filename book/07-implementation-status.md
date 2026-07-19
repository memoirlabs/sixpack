# Implementation Status

This chapter is the honesty check.

## Implemented

### Core Model

- `DatabaseSchema`
- `TableSchema`
- `FieldSpec`
- `LookupSpec`
- `Record`
- `Value`
- `PrimitiveType`
- schema validation

### Format

- `.6` preambles
- `.6` operation rows
- put rows
- delete tombstones
- binary `.6b` v2 cache encoding/decoding
- legacy text `.6b` decode for rebuild migration
- target `engine/state.6pack` pack documented, not implemented

### Store

- local database directory layout
- reverse-sorted chunk naming
- append into reusable `.6` segments
- append full replacement
- same-table write batches
- delete tombstones
- compact recoverable metadata counters
- lazy disk `.6b` persistence with source-hash rebuild checks
- `next_tx` recovery from canonical `.6` operation rows
- `.6b` rebuilds
- id lookup
- declared lookup reads
- live table scans
- live counts
- unique lookup conflict checks
- local workspace read/write coordination across independently opened handles
- synced canonical commits and revision-based cache invalidation
- cross-table incomplete final-operation recovery with clean-tail fail-closed checks
- opt-in table compaction behind `experimental-compaction`

### Runtime

- `Database`
- `db.get(selector)` for current state once
- `db.write(change)` for declared state changes
- `db.write_many(changes)` for same-table batched changes
- `db.get_page_by(...)` for paged declared-lookup reads
- generated `.page(limit)` lookup selectors that preserve the next cursor
- executable typed AI chat and note-taking application contract
- `execute_plan`

### Schema Compiler

- parses `schema!`
- validates schema
- emits table handles
- emits generated `by` selectors
- emits generated `add`/`set`/`edit`/`remove` changes
- emits patch builders
- emits unique lookup keys
- emits page/count selectors
- emits typed TypeScript rows, selectors, keys, and changes

### TypeScript

- generated row types for all schema primitives
- exact TypeScript `bigint` round trips for Rust `i64` values
- checked `int64(...)` conversion from safe numbers and decimal strings
- typed `get`, `write`, and `writeMany` calls
- unique and non-unique lookup selectors
- lookup/table pagination and counts
- add, set, edit, and remove changes
- short-lived Rust bridge execution against canonical `.6` storage

### CLI

- help
- version
- TypeScript schema generation
- internal TypeScript SDK bridge

## Not Implemented

- stable generated API snapshots
- CLI database maintenance commands
- admin UI
- `db.watch(selector)` live subscriptions
- plan JSON serde
- repair/inspect CLI
- crash-hardened default compaction
- `.6x`
- durable cursor format
- single generated `engine/state.6pack` file replacing per-table `.6b` files
