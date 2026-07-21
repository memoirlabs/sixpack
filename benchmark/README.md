# Benchmarks

This package contains local performance comparisons for sixpack behavior that
exists today. Keep benchmark code separate from product runtime code.

## State API vs SQLite

Run from the repository root:

```sh
cargo bench -p sixpack-benchmark --bench crud_vs_sqlite
```

The current benchmark compares basic state access/change behavior for a small
`users` table:

- sixpack `write(add)` vs SQLite `INSERT`
- sixpack `insert_many` vs SQLite transaction insert
- sixpack `get` by id selector vs SQLite `SELECT ... WHERE id = ?`
- sixpack binary projection `count` vs SQLite `COUNT(*)`
- sixpack `write(edit)` vs SQLite `UPDATE`
- sixpack `write_many(edit)` vs SQLite `UPDATE`
- sixpack `write(remove)` vs SQLite `DELETE`
- sixpack `write_many(remove)` vs SQLite `DELETE`

Each sample uses a temporary directory-backed database. The benchmark measures
the current sixpack storage path, including append writes, recoverable metadata,
and generated `.6b` projection maintenance after writes.

The key engine comparison is one-row-at-a-time writes versus same-table
write batches. Batches should append to one `.6` segment and publish one
`.6b` projection. Metadata is recoverable and should not dominate the hot
write path.

## Hot Path vs SQLite

Run from the repository root:

```sh
RUSTFLAGS='-C target-cpu=native' \
  cargo bench -p sixpack-benchmark --bench hot_path -- \
  --sample-size 10 --warm-up-time 0.2 --measurement-time 1.0
```

This benchmark is the more realistic local-application check. It preloads
10,000 rows once, keeps the same live database handle open, then measures
1,000 operations per iteration. Read/count cases stay fixed-size. Write cases
keep mutating the same live handle, so they measure ongoing append/update
behavior instead of database regeneration.

The current benchmark groups are:

- sixpack `get` by id selector vs SQLite indexed `SELECT`
- sixpack binary projection `count` vs SQLite `COUNT(*)`
- sixpack `write(add)` vs SQLite `INSERT`
- sixpack `insert_many` vs SQLite transaction insert
- sixpack `write(edit)` vs SQLite `UPDATE`
- sixpack `write_many(edit)` vs SQLite transaction update

The intended storage comparison is:

- `.6` remains the canonical readable append source.
- `.6b` remains the generated binary lookup/count/read projection.
- hot reads should use the runtime `.6b` map and in-memory row cache.
- hot writes should update the runtime map and materialize compact `.6b`
  lazily, not rebuild the database or rewrite the binary snapshot per row.
- measured loops should not regenerate the database or rebuild `.6b`.

## Cached Reopen vs Plain-Text Rebuild

Run from the repository root:

```sh
cargo bench -p sixpack-benchmark --bench cache_reopen
```

This benchmark creates temporary 10,000-row and 100,000-row databases and
compares three cold-open paths:

- reopening with a current generated `.6b` cache;
- deleting only `.6b`, then reopening and rebuilding it from `.6`.
- reopening an indexed SQLite database and running the same whole-table count.

Dataset creation and cache deletion are outside the measured interval. The
benchmark does not change production cache, locking, revision, or sync
behavior.

Behavioral SQLite parity is tested separately in `tests/sqlite_parity.rs` for
id reads, declared lookups, pagination, scans, counts, edits, deletes, cached
restart, and missing-cache rebuild.

## Agent Workload vs One-File-Per-Conversation JSONL

Run from the repository root:

```sh
cargo bench -p sixpack-benchmark --bench agent_jsonl
```

The fixture contains 100 conversations with 100 messages each. It compares:

- reading one 100-message conversation;
- finding one message by id when its conversation is known;
- finding streaming messages across every conversation;
- reopening storage and reading one history;
- appending and syncing one durable message.

The JSONL append calls `sync_data()` after every line. It still has fewer
safety guarantees than sixpack: no schema validation, duplicate-id check,
revision publication, generated index, or automatic interrupted-tail recovery.
