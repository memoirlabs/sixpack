---
title: AI chat and notes
description: A safe v0.0.1 storage pattern for conversations, messages, streaming responses, and AI-authored notes.
order: 2
---

The tested v0.0.1 path stores conversations, messages, and AI-authored notes through the generated Rust API. It is intended for a small local application with at most two low-traffic processes sharing one local filesystem.

## Store rows, not token events

Persist meaningful lifecycle boundaries:

1. Write the user message with a stable, retry-safe id.
2. Optionally add an assistant message with a `streaming` status.
3. Stream model tokens over SSE, WebSocket, or your application transport.
4. Patch the assistant row with the final body and a `completed` status.
5. Write extracted notes with the conversation id as their source.

```rust
db.write(messages::add(user_row))?;
db.write(messages::add(pending_assistant))?;

// Stream tokens outside the database.

db.write(messages::edit(
    messages::key::id(message_id),
    messages::Patch::new()
        .body(final_text)
        .status("completed"),
))?;
```

Do not append one database row for every token. It creates unnecessary durable writes and leaves more partial state to clean up after a crash.

## Read a conversation in pages

Declared non-unique lookup selectors expose `.page(limit)`. The result contains typed rows and the next cursor.

```rust
let (rows, next) = db.get(
    messages::by::conversation_id(chat_id).page(100),
)?;

let (more, next) = db.get(
    messages::by::conversation_id(chat_id)
        .page(100)
        .cursor(next.unwrap()),
)?;
```

Use sortable message ids so the lookup's id ordering is also useful chronological ordering. Cursors reflect current live rows; they are not frozen snapshots.

## Application responsibilities

- Validate user ownership, message roles, statuses, and referenced conversation ids.
- Give every retried write a stable id. Duplicate adds fail instead of silently duplicating data.
- Run the synchronous database API on a blocking worker in an async server.
- Keep cross-table actions idempotent because there are no cross-table transactions.
- Delete child messages and notes explicitly before or after deleting a conversation.
- Back up the entire database directory while no writes are active.

## Failure behavior

Generated caches can be rebuilt from schema plus canonical `.6` data. A dirty workspace may repair an incomplete final line before the next append. Clean-state corruption and malformed canonical rows fail closed and are left untouched for inspection.

The [complete executable guide](https://github.com/memoirlabs/sixpack/blob/main/packages/docs/ai-chat-notes.md) includes the schema, `build.rs`, retries, deletion flow, backup guidance, and release checklist.
