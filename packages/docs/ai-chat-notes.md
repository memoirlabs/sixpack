# AI Chat and Notes Guide

This is the application contract for using sixpack `v0.0.1` in a local AI
assistant that stores conversations, messages, and notes. The executable
version of this guide lives in
[`apps/test-lab/ai-chat-notes`](../../apps/test-lab/ai-chat-notes/README.md).

## Supported deployment profile

Use this release when all of the following are true:

- the application and database run on one machine;
- the database root is on a local filesystem;
- at most two low-traffic processes open the same workspace;
- blocking durable writes around 12–14 ms are acceptable;
- the application can stream model tokens over its own HTTP, SSE, WebSocket,
  or desktop channel and persist completed state separately.

Do not use `v0.0.1` as a multi-host database, on a network filesystem, or when
you need SQL, cross-table transactions, full-text search, foreign keys, or live
database subscriptions.

## What ships in `v0.0.1`

| Need | Public API | Behavior |
| --- | --- | --- |
| Create a row once | `db.write(table::add(row))` | Fails on an existing id or unique lookup. |
| Create or replace | `db.write(table::set(row))` | Inserts or replaces the complete row. |
| Read by id | `db.get(table::by::id(id))` | Returns `Option<Row>`. |
| Read by lookup | `db.get(table::by::field(value))` | Returns one typed row for a unique lookup or up to 1,000 typed rows for a non-unique lookup. |
| Page a lookup | `db.get(table::by::field(value).page(limit))` | Returns `(Vec<Row>, Option<String>)`. Pass the returned cursor to `.cursor(...)`. |
| Scan a table | `db.get(table::all().limit(limit))` | Returns `(Vec<Row>, Option<String>)`. |
| Count a table | `db.get(table::count())` | Returns the current live row count. |
| Patch one row | `db.write(table::edit(table::key::id(id), patch))` | Preserves fields not present in the patch and rejects id changes. |
| Remove one row | `db.write(table::remove(table::key::id(id)))` | Appends a tombstone. |
| Batch one table | `db.write_many(&changes)` | Validates the batch first and performs one same-table append/sync. |

`db.watch(...)` is not implemented. Use application notifications after local
writes or polling from another process. `.6x` full-text search is also not
implemented; model important access paths as schema lookups.

## Install from the release tag

Until crates are published separately, depend on the Git tag:

```toml
[dependencies]
sixpack = { git = "https://github.com/memoirlabs/sixpack", tag = "v0.0.1" }

[build-dependencies]
sixpack-schema-compiler = { git = "https://github.com/memoirlabs/sixpack", tag = "v0.0.1" }
```

Normal chat and note storage does not need `experimental-compaction`.

## Define the application schema

Create `schema.sixpack`:

```rust
schema! {
  conversations {
    id id
    owner_id id
    title text
    created_at int
    updated_at int
    archived bool

    lookup owner_id
    lookup updated_at
  }

  messages {
    id id
    conversation_id id
    role text
    body text
    status text
    model text
    created_at int
    sequence int

    lookup conversation_id
    lookup status
  }

  notes {
    id id
    owner_id id
    title text
    body text
    source_kind text
    source_id text
    created_at int
    updated_at int

    lookup owner_id
    lookup source_id
    lookup updated_at
  }
}
```

The primitive types are `id`, `text`, `int`, `float`, and `bool`. Every field is
required. There are no optional, enum, list, JSON, blob, or relation types in
this release. Use explicit values such as an empty `model` for user messages,
and validate domain values such as `role` and `status` in application code.

Suggested domain values:

- `role`: `user`, `assistant`, `system`, or `tool`;
- `status`: `streaming`, `completed`, `cancelled`, or `error`;
- `source_kind`: `conversation`, `message`, `manual`, or another value owned by
  the application.

These are conventions, not database-enforced enums.

## Generate the typed Rust API

Add `build.rs`:

```rust
use std::env;
use std::fs;
use std::path::PathBuf;

use sixpack_schema_compiler::{compile_schema, emit_raw_rust};

fn main() {
    println!("cargo:rerun-if-changed=schema.sixpack");
    let source = fs::read_to_string("schema.sixpack").expect("read schema");
    let ir = compile_schema(&source).expect("compile schema");
    let generated = emit_raw_rust(&ir);
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("sixpack_schema.rs"), generated).expect("write SDK");
}
```

Load it from application code:

```rust
include!(concat!(env!("OUT_DIR"), "/sixpack_schema.rs"));
use sixpack_generated_schema as sdk;
```

The generated SDK contains:

- one typed `Row` per table;
- `Patch` builders for non-id fields;
- `by` selectors for id and declared lookups;
- `key` constructors for id and unique lookups;
- `add`, `set`, `edit`, and `remove` changes;
- typed lookup/table pagination and counts;
- table handles through the generated `GeneratedTables` trait.

The lightweight `sixpack::schema!` macro only constructs runtime schema
metadata. It does not emit `Row`, `Patch`, `by`, `key`, or change helpers. Use
the build-time compiler above for the typed application API.

## Open and initialize the database

```rust
use sixpack::Database;

let db = Database::open_local_with_schema(
    "./local-data",
    "assistant",
    sdk::database_schema(),
);
db.init()?;
```

The first argument is the parent directory. The second is the workspace name,
so this example stores data under `./local-data/assistant/`. `init()` is safe to
call during startup: it creates missing layout, validates existing table
headers, rebuilds generated caches when necessary, and publishes recoverable
metadata.

Use the same schema for every process opening a workspace.

## Start a conversation

Use `add` when an id must be created once:

```rust
db.write(sdk::conversations::add(sdk::conversations::Row {
    id: conversation_id.clone(),
    owner_id: user_id.clone(),
    title: "New conversation".to_owned(),
    created_at: now,
    updated_at: now,
    archived: false,
}))?;
```

There are no foreign keys. Before writing a message, the application is
responsible for checking that the conversation exists and that the current
user owns it.

## Persist a user message

Create a stable message id before calling the model. Reusing the same id makes
request retries detectable:

```rust
db.write(sdk::messages::add(sdk::messages::Row {
    id: user_message_id.clone(),
    conversation_id: conversation_id.clone(),
    role: "user".to_owned(),
    body: prompt.clone(),
    status: "completed".to_owned(),
    model: String::new(),
    created_at: now,
    sequence: next_sequence,
}))?;
```

`add` returns an error if the id already exists. On a retry, read that id and
verify its conversation/body rather than creating another row.

## Stream and finish an assistant response

Token delivery is an application transport concern. A safe lifecycle is:

1. Durably persist the user message.
2. Optionally add an assistant placeholder with `status = "streaming"`.
3. Stream tokens from the model to the client without a database write per
   token.
4. Patch the assistant row once with the final body and terminal status.

```rust
db.write(sdk::messages::add(sdk::messages::Row {
    id: assistant_message_id.clone(),
    conversation_id: conversation_id.clone(),
    role: "assistant".to_owned(),
    body: String::new(),
    status: "streaming".to_owned(),
    model: model_name.clone(),
    created_at: now,
    sequence: next_sequence + 1,
}))?;

// Stream tokens through SSE/WebSocket/etc. Keep the partial buffer in memory.

db.write(sdk::messages::edit(
    sdk::messages::key::id(assistant_message_id),
    sdk::messages::Patch::new()
        .body(final_text)
        .status("completed"),
))?;
```

If the process stops before the final patch, restart logic can find
`status = "streaming"` messages and mark them `cancelled` or resume them using
application-specific model/request state. sixpack does not resume model calls.

Persisting every token would turn each token into a synced filesystem commit.
That is safe but needlessly slow for normal chat UX.

## Read and page a chat history

For a small history of at most 1,000 messages:

```rust
let messages = db.get(
    sdk::messages::by::conversation_id(conversation_id.clone()),
)?;
```

For a long history, use typed lookup pagination:

```rust
let (first, next_cursor) = db.get(
    sdk::messages::by::conversation_id(conversation_id.clone()).page(100),
)?;

let (second, next_cursor) = match next_cursor {
    Some(cursor) => db.get(
        sdk::messages::by::conversation_id(conversation_id)
            .page(100)
            .cursor(cursor),
    )?,
    None => (Vec::new(), None),
};
```

Treat cursors as opaque and short-lived. They are offsets over current live
rows, not snapshot tokens; inserts or deletes before the cursor can shift a
later page.

Lookup results are ordered lexicographically by row id, not by `created_at` or
`sequence`. Use monotonically sortable message ids when page order must match
conversation order, and use `sequence` as an application-level assertion and
display sort key. Arbitrary random ids require collecting and sorting in the
application and are not suitable for stable incremental pagination.

## Capture and edit notes

An AI-extracted note can retain its source without a relation type:

```rust
db.write(sdk::notes::add(sdk::notes::Row {
    id: note_id.clone(),
    owner_id: user_id.clone(),
    title: "Release decisions".to_owned(),
    body: extracted_note,
    source_kind: "conversation".to_owned(),
    source_id: conversation_id.clone(),
    created_at: now,
    updated_at: now,
}))?;

let conversation_notes = db.get(sdk::notes::by::source_id(conversation_id))?;

db.write(sdk::notes::edit(
    sdk::notes::key::id(note_id),
    sdk::notes::Patch::new()
        .title("Reviewed release decisions")
        .body(reviewed_body)
        .updated_at(later),
))?;
```

For a manual note with no source object, use an application convention such as
`source_kind = "manual"` and `source_id = ""`. Optional fields are not yet
available.

## Batch behavior

Use `write_many` to validate and append several changes to one table:

```rust
db.write_many(&[
    sdk::messages::add(first_imported_message),
    sdk::messages::add(second_imported_message),
])?;
```

Rules:

- every change must belong to the same table;
- insert changes cannot be mixed with edit/set/remove changes;
- one row id cannot be touched twice in one batch;
- schema and uniqueness validation happen before the append;
- the batch uses one table append and one sync.

This is not a general transaction and not a cross-table transaction. A storage
failure during the append may leave a complete prefix of operations; recovery
only removes an incomplete final operation. Design imports and retries around
stable ids.

## Two access points

Two independently opened handles/processes on the same local filesystem share
an advisory workspace lock and revision marker:

- reads can run together;
- writes are serialized across all tables;
- a process invalidates stale generated/row caches after another commit;
- an interrupted dirty commit is recovered before the next write;
- unexpected clean corruption fails closed.

Do not share the workspace through NFS, SMB, cloud-synced folders, or between
hosts. Do not copy a live workspace while a writer is active. For backup, stop
writes and copy the entire workspace directory.

The API is synchronous/blocking. In Tokio or another async server, run database
operations on a blocking thread or a small dedicated database worker rather
than blocking the async executor.

## Delete behavior

Deleting a conversation does not delete its messages or notes automatically.
There are no foreign keys or cascade rules. The application must enumerate and
remove dependent rows explicitly, preferably as separate same-table batches.
Because those table batches are not one cross-table transaction, make cleanup
idempotent and resumable.

## Corruption and recovery contract

- `.6` files are canonical rows.
- `.6b` files are derived caches and are rebuilt if missing, stale, or corrupt.
- missing `sixpack.toml` counters are recovered from canonical transaction ids.
- a dirty revision permits truncation of non-newline tails across tables.
- a non-newline tail under a clean revision is reported and left untouched.
- malformed complete rows, table/profile header mismatches, corrupt revision
  markers, and invalid metadata counters return errors instead of discarding
  canonical data.

There is no public repair/inspect CLI in `v0.0.1`. Preserve the workspace and
diagnose the reported file before editing canonical data manually.

## Physical layout

Rows are table-oriented, not conversation-oriented:

```txt
assistant/
  sixpack.toml
  tables/
    conversations/
      zzz.6
    messages/
      zzz.6
      zzy.6
    notes/
      zzz.6
  engine/
    workspace.lock
    revision
    conversations.6b
    messages.6b
    notes.6b
```

All conversations share `tables/conversations/*.6`; all messages share
`tables/messages/*.6`. The indexed `conversation_id` field selects a history.
Chunks roll over by table size, never one file per conversation.

## Release checklist for an AI application

- Generate ids in the application and keep them stable across retries.
- Use sortable message ids if lookup pagination must preserve chat order.
- Validate ownership, foreign keys, roles, statuses, and sequence numbers in
  the application.
- Persist the user message before starting the model request.
- Stream tokens outside the database; durably patch the completed assistant
  message.
- Treat `streaming` rows as recoverable application state after restart.
- Use `add` for retry detection and `set` only when replacement is intended.
- Keep `write_many` within one table and do not assume all-or-nothing commit.
- Run blocking database calls away from an async executor.
- Keep the workspace on one local filesystem with no more than two low-traffic
  processes.
- Stop writers before backup and copy the entire workspace.
- Surface database errors; do not automatically delete canonical files.

## Executable verification

Run the concrete chat/note contract:

```sh
cargo test -p ai-chat-notes
```

It verifies:

- typed generated rows/selectors/patches/changes compile;
- user, assistant, and tool messages round-trip;
- tabs, newlines, Unicode, and emoji survive cold reopen;
- a streaming assistant placeholder can be completed with a patch;
- typed conversation lookup pagination returns and consumes cursors;
- AI-extracted notes can be queried and edited;
- independent handles observe each other's chat/note commits;
- duplicate message retries do not create a second row;
- an invalid same-table batch writes none of its rows.

The storage suite separately verifies repeated two-OS-process races, synced
writes, crash tails, missing/corrupt metadata, corrupt caches, malformed
canonical rows, and cold reopen behavior.
