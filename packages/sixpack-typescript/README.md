# @sixpack/db

Typed TypeScript API for the local sixpack Rust engine.

This package targets Node.js and Bun local applications. It is not a browser
database package because it launches the local `sixpack` executable.

The generated scalar mapping is exact and intentionally small:

| Schema | TypeScript |
| --- | --- |
| `id` | `string` |
| `text` | `string` |
| `int` | `bigint` |
| `float` | finite `number` |
| `bool` | `boolean` |

Generated rows are readonly snapshots. Use `edit(...)` or `set(...)` to persist
changes instead of mutating a returned object.

Generate a schema module:

```sh
sixpack generate typescript schema.sixpack > sixpack-schema.ts
```

Use it from TypeScript:

```ts
import { Database, int64 } from "@sixpack/db";
import { messages, schema } from "./sixpack-schema.js";

const db = new Database({
  root: "./data",
  workspace: "chat",
  schema,
  schemaPath: "./schema.sixpack",
});

await db.init();

await db.write(messages.add({
  id: "m1",
  conversation_id: "c1",
  sender_id: "u1",
  body: "hello",
  created_at: int64(Date.now()),
}));

const message = await db.get(messages.by.id("m1"));
```

Schema `int` fields are exact TypeScript `bigint` values, matching Rust `i64`
without reducing its range. Use `123n` directly or `int64(Date.now())` for
ordinary safe numbers. `int64(...)` also accepts decimal strings and rejects
unsafe numbers or values outside the signed 64-bit range.

The client uses a short-lived `sixpack bridge` process for each operation. This
prioritizes a working, correct API; a persistent transport can replace it later
without changing the generated table API.
