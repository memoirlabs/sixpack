# Note Taking Playground

Interactive test-lab app for exercising sixpack as a real local database.

This is separate from `note-taking-init`, which stays the minimal untouched
compiler/init example. The playground is intentionally for experiments:

- create notes through a tiny browser UI,
- add notes repeatedly with an editor that clears after each write,
- edit existing rows only after selecting Edit,
- delete rows through the sixpack runtime API,
- watch read-after-write timing,
- see the database state refresh automatically.

## Frontend

The demo frontend is the canonical Notes starter template at
[`packages/sixpack-cli/templates/notes/static/index.html`](../../../packages/sixpack-cli/templates/notes/static/index.html):
one plain HTML file with CSS and vanilla JavaScript. Rust embeds it into the
example binary with `include_str!` and serves it from the same small local
process as the CRUD API.
There is no React, Node runtime, frontend framework, bundler, dependency install,
or separate frontend server.

The automatically generated `projection.html` is a different surface: it is a
generic, read-only view produced from the current database state.

Run:

```sh
cargo run -p note-taking-playground -- \
  --database target/test-lab/note-taking-playground \
  --reset
```

Then open:

```txt
http://127.0.0.1:4766/
```

Options:

```txt
--host <host>    bind host, default 127.0.0.1
--port <port>    bind port, default 4766
--database <path> final database directory, default is temporary
--reset           remove that database directory before startup
```

The database lives under:

```txt
target/test-lab/note-taking-playground/
```

Each mutation also rewrites a self-contained, read-only projection:

```txt
target/test-lab/note-taking-playground/projection.html
```

Open that file directly. It embeds the current rows and reloads once per second;
there is no upload step or projection server.

The database write is authoritative. A projection-refresh error is returned as
`projection_error` in the mutation timing payload and logged, but it does not
misreport an already committed database write as failed.

This app is not the public admin UI. It is a focused playground for speed,
reactivity, and storage-shape experiments.

Pass `--port 0` when the default port is busy. The startup line prints the
actual OS-selected URL.
