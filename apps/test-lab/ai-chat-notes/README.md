# AI Chat + Notes Contract

Executable `v0.0.1` integration example for the build-time generated API.
It covers conversations, user/assistant/tool messages, assistant completion,
typed lookup pagination, note capture, duplicate retries, independent handles,
and cold reopen behavior.

Run the tests:

```sh
cargo test -p ai-chat-notes
```

Run the demo against an explicit disposable directory:

```sh
cargo run -p ai-chat-notes -- \
  --database target/test-lab/ai-chat-quickstart
```

The demo writes a self-contained data view to:

```txt
target/test-lab/ai-chat-quickstart/projection.html
```

The schema is in `schema.sixpack`; `build.rs` compiles it into typed Rust rows,
selectors, patches, changes, and table handles under Cargo's `OUT_DIR`.
Projection errors are reported separately and never turn an already committed
database write into a failed write result.

`--database` is the final database directory, matching the CLI. Running the
demo again against the same directory reconnects safely without duplicating its
fixed example rows. Without the option, the demo uses a temporary directory.
