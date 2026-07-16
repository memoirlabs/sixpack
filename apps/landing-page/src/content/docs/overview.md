---
title: Overview
description: What sixpack is, where it fits, and what is implemented in v0.0.1.
order: 1
---

sixpack is a local-first table database for Rust applications and tools. The canonical data stays in readable files, while generated binary indexes provide the fast path for ids, lookups, counts, and edits.

## The basic model

```text
schema.sixpack  logical schema
tables/*.6      readable source data
engine/*.6b     generated lookup caches
sixpack.toml    recoverable engine metadata
```

Applications define tables, fields, and lookups. sixpack validates typed changes against that schema, appends durable row operations to `.6` files, and maintains rebuildable `.6b` indexes.

## What ships now

- Typed schema primitives and row validation.
- Append-only `.6` put and delete operations.
- Generated `.6b` row-pointer and lookup caches.
- `db.get`, `db.write`, and same-table `db.write_many`.
- Add, replace, patch, remove, count, scan, and declared lookup operations.
- Paged lookup reads for long conversation histories.
- Shared locking and cache revisions for two low-traffic processes on one local filesystem.
- Synced canonical commits and interrupted-tail recovery.
- Build-time schema parsing, validation, and generated Rust APIs.
- CLI help and version commands.

## What it is not

- It is not a SQL engine or hosted database service.
- It does not provide cross-table transactions, foreign keys, or cascade deletion.
- It is not designed for network filesystems, multiple hosts, or heavy write concurrency.
- `db.watch`, full-text search, the admin UI, and repair commands are planned rather than shipped.

## Safety profile

Normal builds coordinate local processes, sync canonical writes, publish cache revisions, rebuild stale generated indexes, and recover an interrupted final row when the workspace is marked dirty. Unexpected corruption in canonical data fails closed.

Compaction is experimental and remains behind the `experimental-compaction` Cargo feature. It is not required for ordinary chat or note reads and writes.
