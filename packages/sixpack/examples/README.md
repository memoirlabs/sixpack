# Examples

Start with the small schema-first chat example:

```sh
cargo run -p sixpack --example chat_schema -- \
  --database target/test-lab/chat-quickstart
```

It initializes a real database, writes user/conversation/message rows, and
prints two paths:

```txt
database target/test-lab/chat-quickstart
projection target/test-lab/chat-quickstart/projection.html
```

Open the projection directly. No server or upload is involved.

For the generated typed API and a fuller assistant lifecycle:

```sh
cargo test -p ai-chat-notes
cargo run -p ai-chat-notes -- \
  --database target/test-lab/ai-chat-quickstart
```

For interactive CRUD and live projection refresh:

```sh
cargo run -p note-taking-playground -- \
  --reset \
  --database target/test-lab/notes-quickstart
```

`--database` always names the final database directory, matching
`sixpack init <database>`. Without it, each example uses a new temporary
directory.
