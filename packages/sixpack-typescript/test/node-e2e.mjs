import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Database, createTable, int64 } from "../dist/index.js";

const users = createTable()({
  name: "users",
  fields: {
    id: "id",
    name: "text",
    email: "text",
    is_ai_user: "bool",
  },
  lookups: {
    id: { kind: "id", unique: true },
    email: { kind: "text", unique: true },
  },
});

const conversations = createTable()({
  name: "conversations",
  fields: {
    id: "id",
    owner_id: "id",
    title: "text",
    created_at: "int",
  },
  lookups: {
    id: { kind: "id", unique: true },
    owner_id: { kind: "id", unique: false },
    created_at: { kind: "int", unique: false },
  },
});

const messages = createTable()({
  name: "messages",
  fields: {
    id: "id",
    conversation_id: "id",
    sender_id: "id",
    body: "text",
    created_at: "int",
  },
  lookups: {
    id: { kind: "id", unique: true },
    conversation_id: { kind: "id", unique: false },
    created_at: { kind: "int", unique: false },
  },
});

const schema = {
  hash: "0998dc171e3b836c",
  tables: { users, conversations, messages },
};
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "../..");
const root = await mkdtemp(resolve(tmpdir(), "sixpack-node-e2e-"));
const db = new Database({
  root,
  workspace: "node",
  schema,
  schemaPath: resolve(repositoryRoot, "packages/sixpack/examples/chat_schema.sixpack"),
  binaryPath: resolve(repositoryRoot, "target/debug/sixpack"),
});

try {
  await db.init();
  await db.write(
    messages.add({
      id: "node-message",
      conversation_id: "node-conversation",
      sender_id: "node-user",
      body: "Node executes the compiled package",
      created_at: int64(Date.now()),
    }),
  );
  const row = await db.get(messages.by.id("node-message"));
  assert.equal(row.body, "Node executes the compiled package");
  assert.equal(typeof row.created_at, "bigint");
  console.log("Node executed the compiled @sixpack/db package against Rust storage");
} finally {
  await rm(root, { recursive: true, force: true });
}
