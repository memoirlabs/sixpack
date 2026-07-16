# Changelog

## 0.0.1

First application-testing release of the local directory database runtime.

### Safe default path

- Coordinate reads and writes across two low-traffic local processes.
- Sync canonical `.6` appends before publishing a clean commit revision.
- Invalidate stale caches after another process commits.
- Recover from a dirty revision and discard an incomplete final operation.
- Recover dirty tails across every table before publishing another commit.
- Reject unexpected incomplete tails in clean workspaces without modifying them.
- Rebuild corrupt generated `.6b` caches from canonical `.6` rows.
- Fail loudly on corrupt revision metadata, canonical rows, or mismatched table headers.
- Keep all conversations in shared table chunks, indexed by `conversation_id`.
- Page declared lookup results with `Database::get_page_by`.
- Page generated non-unique lookup selectors with `.page(limit)` while
  preserving the next cursor.
- Ship an executable typed AI chat/notes contract and public integration/API
  documentation.

### Explicitly experimental

- Table compaction requires the `experimental-compaction` Cargo feature.
- `db.watch`, repair/inspect CLI commands, full-text search, and the admin UI
  remain planned.
- Multi-host and network-filesystem access are unsupported.

### Verification

- Chat layout, escaping, size-based rollover, pagination, and cold reopen.
- Generated conversation/message/note rows, assistant completion, retries, and
  typed lookup pagination.
- Concurrent reads and writes through independently opened handles.
- Two independent processes writing the same chat table without lost rows.
- Repeated two-process cross-table write races with unique transaction ids.
- Full workspace formatting, checks, tests, Clippy, and Criterion smoke tests.
