import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { Database, SixpackError, int64 } from "@sixpack/db";
import {
  messages,
  schema,
  users,
  type MessagesRow,
} from "./chat-schema.ts";

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../..");
const root = await mkdtemp(resolve(tmpdir(), "sixpack-typescript-e2e-"));

const db = new Database({
  root,
  workspace: "chat",
  schema,
  schemaPath: resolve(repositoryRoot, "packages/sixpack/examples/chat_schema.sixpack"),
  binaryPath: resolve(repositoryRoot, "target/debug/sixpack"),
});

if (false) {
  const snapshot = {} as MessagesRow;
  // @ts-expect-error returned rows are immutable snapshots
  snapshot.body = "mutated";
  // @ts-expect-error keys from another generated table cannot be used here
  messages.remove(users.key.id("u1"));
  // @ts-expect-error generated rows require every schema field
  users.add({ id: "u1", name: "Ada" });
  messages.add({
    id: "m0",
    conversation_id: "c0",
    sender_id: "u0",
    body: "bad integer type",
    // @ts-expect-error exact int fields are bigint, not lossy JS numbers
    created_at: 1,
  });
  // @ts-expect-error edits require at least one changed field
  messages.edit(messages.key.id("m0"), {});
  db.writeMany([
    // @ts-expect-error writeMany rejects changes from different tables
    users.remove(users.key.id("u1")),
    // @ts-expect-error writeMany rejects changes from different tables
    messages.remove(messages.key.id("m0")),
  ]);
}

try {
  await db.init();

  const userWrite = await db.write(
    users.add({
      id: "u1",
      name: "Ada",
      email: "ada@example.com",
      is_ai_user: false,
    }),
  );
  assert.equal(userWrite.operation, "put");
  assert.equal(userWrite.txId, 1n);

  const byEmail = await db.get(users.by.email("ada@example.com"));
  assert.deepEqual(byEmail, {
    id: "u1",
    name: "Ada",
    email: "ada@example.com",
    is_ai_user: false,
  });

  await db.writeMany([
    messages.add({
      id: "m1",
      conversation_id: "c1",
      sender_id: "u1",
      body: "first",
      created_at: int64(1_700_000_000_001),
    }),
    messages.add({
      id: "m2",
      conversation_id: "c1",
      sender_id: "u1",
      body: "second",
      created_at: int64("1700000000002"),
    }),
  ]);

  const page = await db.get(messages.by.conversation_id("c1").page(1));
  assert.equal(page.rows.length, 1);
  assert.equal(page.rows[0]?.created_at, 1_700_000_000_001n);
  assert.notEqual(page.nextCursor, null);

  const secondPage = await db.get(
    messages.by.conversation_id("c1").page(1).cursor(page.nextCursor!),
  );
  assert.equal(secondPage.rows[0]?.id, "m2");

  const byCreatedAt = await db.get(
    messages.by.created_at(int64("1700000000001")),
  );
  assert.deepEqual(byCreatedAt.map((row) => row.id), ["m1"]);

  const scan = await db.get(messages.all().limit(1));
  assert.equal(scan.rows.length, 1);
  assert.notEqual(scan.nextCursor, null);
  const rest = await db.get(messages.all().limit(10).cursor(scan.nextCursor!));
  assert.equal(rest.rows.length, 1);

  await db.write(
    messages.set({
      id: "m1",
      conversation_id: "c1",
      sender_id: "u1",
      body: "set replacement",
      created_at: int64(1_700_000_000_001),
    }),
  );
  assert.equal(
    (await db.get(messages.by.id("m1")))?.body,
    "set replacement",
  );

  await db.write(
    messages.edit(messages.key.id("m1"), { body: "updated" }),
  );
  assert.equal((await db.get(messages.by.id("m1")))?.body, "updated");
  assert.equal(await db.get(messages.count()), 2n);

  assert.throws(
    () => int64(Number.MAX_SAFE_INTEGER + 1),
    (error: unknown) =>
      error instanceof SixpackError && error.code === "type_mismatch",
  );
  assert.equal(int64("9223372036854775807"), 9_223_372_036_854_775_807n);
  assert.throws(
    () => int64("9223372036854775808"),
    (error: unknown) =>
      error instanceof SixpackError && error.code === "integer_out_of_range",
  );

  await db.writeMany([
    messages.edit(messages.key.id("m1"), { body: "batched update" }),
    messages.remove(messages.key.id("m2")),
  ]);
  assert.equal(
    (await db.get(messages.by.id("m1")))?.body,
    "batched update",
  );
  assert.equal(await db.get(messages.by.id("m2")), null);

  await assert.rejects(
    db.write(
      users.add({
        id: "u2",
        name: "Duplicate",
        email: "ada@example.com",
        is_ai_user: false,
      }),
    ),
    (error: unknown) =>
      error instanceof SixpackError && error.code === "already_exists",
  );

  const wrongSchema = new Database({
    root,
    workspace: "wrong-schema",
    schema: { ...schema, hash: "wrong" },
    schemaPath: resolve(
      repositoryRoot,
      "packages/sixpack/examples/chat_schema.sixpack",
    ),
    binaryPath: resolve(repositoryRoot, "target/debug/sixpack"),
  });
  await assert.rejects(
    wrongSchema.init(),
    (error: unknown) =>
      error instanceof SixpackError && error.code === "schema_mismatch",
  );

  const missingBinary = new Database({
    root,
    workspace: "missing-binary",
    schema,
    schemaPath: resolve(
      repositoryRoot,
      "packages/sixpack/examples/chat_schema.sixpack",
    ),
    binaryPath: resolve(root, "does-not-exist"),
  });
  await assert.rejects(
    missingBinary.init(),
    (error: unknown) =>
      error instanceof SixpackError && error.code === "bridge_failed",
  );

  console.log("TypeScript API completed the full typed CRUD and error contract");
} finally {
  await rm(root, { recursive: true, force: true });
}
