# AI Chat + Notes Contract

Executable `v0.0.1` integration example for the build-time generated API.
It covers conversations, user/assistant/tool messages, assistant completion,
typed lookup pagination, note capture, duplicate retries, independent handles,
and cold reopen behavior.

Run the tests:

```sh
cargo test -p ai-chat-notes
```

Run the demo against a disposable directory:

```sh
cargo run -p ai-chat-notes -- --out /tmp/sixpack-ai-demo
```

The schema is in `schema.sixpack`; `build.rs` compiles it into typed Rust rows,
selectors, patches, changes, and table handles under Cargo's `OUT_DIR`.
