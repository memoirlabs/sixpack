# Testing

sixpack is the database product, so tests must exercise local directory-backed
behavior directly.

## Core Rule

Use temporary directories for data-bearing tests.

Never write disposable test data to:

```txt
.sixpack
.data
repo root paths
real user workspace paths
```

## Mental Model

```txt
A sixpack database instance = one directory.
A test database = one temporary directory.
Resetting the database = deleting that directory.
```

## Test Layers

### Unit Tests

Use for pure deterministic pieces:

- schema validation
- primitive type mapping
- value parsing
- row encoding
- row pointer parsing
- chunk path encoding

### Integration Tests

Use temporary directories and real store/runtime calls:

- open database
- initialize layout
- write add/set/edit/remove changes
- get by id selector
- get by lookup selector
- get page and count selectors
- rebuild caches
- verify unique lookup conflicts
- interleave paged chat histories and verify conversation isolation
- verify interleaved chats share table chunks and escaped values stay one line
- force message chunk rollover and verify it is size-based, not conversation-based
- race cloned handles across tables and verify transaction uniqueness
- read through one handle while another handle commits
- race two OS processes against one workspace and verify cold reopen
- invalidate a warm handle after another handle commits
- recover from a dirty revision and incomplete final `.6` line
- recover a dirty tail before a write to a different table
- reject malformed complete rows, mismatched headers, and clean partial tails
- rebuild a corrupt `.6b` cache without changing canonical `.6` data
- exercise typed conversation/message/note flows, assistant completion,
  duplicate retries, independent handles, lookup pagination, and cold reopen

### Contract Tests

Use for public behavior:

- CLI output
- file format stability
- generated API behavior once stable

### Snapshot Tests

Use only for stable reviewed output:

- CLI help
- generated schema output
- stable format rendering

## Required Checks

Run from repo root:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Focused concurrency and chat checks:

```sh
cargo test -p sixpack-store two_process_writes_are_serialized_and_recoverable
cargo test -p sixpack-store cloned_handle_serializes_cross_table_writes
cargo test -p sixpack-store dirty_revision_discards_an_incomplete_tail
cargo test -p sixpack paged_chat_histories_stay_isolated_and_survive_reopen
cargo test -p sixpack chat_storage_uses_shared_table_chunks
cargo test -p sixpack message_chunks_roll_over_by_size_not_by_conversation
```
