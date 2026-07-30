# sixpack Commands

The CLI covers the local setup loop: create a runnable starter, initialize a
schema-backed database, generate typed APIs, and refresh the no-server data
projection.

## Install

From this repository:

```sh
cargo install --path apps/sixpack
sixpack --version
```

## Fastest setup

For a guided Rust project:

```sh
sixpack create
```

The interactive flow asks for a project path and template, builds the generated
binary, starts the Notes template when selected, and prints a clickable local
URL. The non-interactive form is:

```sh
sixpack create ./my-notes --template notes
sixpack create ./my-ai-demo --template ai
sixpack create ./topcoat-sixpack --template topcoat
sixpack create ./my-data --template minimal
```

`--no-start` creates and builds the project without launching it. The Notes
command stays attached like a normal development server; press Ctrl-C to stop.

The npm launcher is not published yet. `bunx sixpack create` is the intended
future packaging shape, while installed builds use `sixpack create`.

For a schema-first database without a starter app, put a `schema.sixpack` in
the current directory:

```rust
schema! {
  notes {
    id id
    title text
    body text
    updated_at int

    lookup updated_at
  }
}
```

Then run:

```sh
sixpack init
```

That initializes `./data` and writes `./data/projection.html`. Open the HTML
file directly; it does not upload data or require a server.

The defaults are deliberately predictable:

```txt
schema:   ./schema.sixpack
database: ./data
```

## `sixpack init`

Initialize a database from a schema and generate its first projection:

```sh
sixpack init
sixpack init ./local-data/assistant
sixpack init ./local-data/assistant --schema ./schemas/assistant.sixpack
```

The database argument is the final workspace directory, not its parent. Calling
`init` again is safe: it validates the schema and existing table headers,
creates missing layout, and rebuilds generated caches as needed. It does not
insert example rows.

## `sixpack create`

Create a complete Rust starter project:

```sh
sixpack create
sixpack create ./my-notes --template notes
sixpack create ./my-ai-demo --template ai
sixpack create ./topcoat-sixpack --template topcoat
sixpack create ./my-data --template minimal --no-start
```

The Notes template contains the dependency-free HTML/CSS/JavaScript CRUD app,
its Rust HTTP/API binary, `schema.sixpack`, and a local database that generates
`data/projection.html`. Chat app contains the same no-framework frontend shape,
a deterministic local `/api/ai/ping` endpoint, and a message table that stores
both `user: ping` and `assistant: ping`. Minimal contains the schema, a small Rust
binary, and the generated projection without a CRUD server.

Topcoat contains one server-rendered notes page, Topcoat-discovered form
routes, a `Topcoat.toml` project marker, and a Sixpack-backed notes table. The
generated manifest pins Topcoat 0.5.0 because the framework is currently
experimental. Install `topcoat-cli` 0.5.0 and run `topcoat dev` inside the
project for Topcoat's normal compile and live-reload workflow; `cargo run`
remains available for a one-off server run.

## `sixpack generate`

Generate a typed Rust or TypeScript schema module:

```sh
sixpack generate rust
sixpack generate rust --out generated/schema.rs
sixpack generate typescript --out generated/schema.ts
sixpack generate typescript ./schemas/assistant.sixpack
```

The schema defaults to `./schema.sixpack`. Without `--out`, generated source is
written to stdout for normal shell composition.

Rust applications can keep using the schema compiler from `build.rs` when they
want Cargo to regenerate into `OUT_DIR`; the CLI is the shortest path for
inspection, checked-in generated modules, scripts, and non-Cargo workflows.

## `sixpack project`

Regenerate the self-contained HTML view for an initialized database:

```sh
sixpack project
sixpack project ./local-data/assistant
```

The database defaults to `./data`. `project` only reads canonical `.6` files
and rewrites `<database>/projection.html`; it does not change database rows.

The test-lab examples refresh the projection after their own writes. For other
applications, call `sixpack project` after external writes when a fresh HTML
snapshot is wanted.

## Help and version

```sh
sixpack help
sixpack help create
sixpack help init
sixpack help generate
sixpack help project
sixpack --version
```

## Exit codes

- `0`: command completed.
- `1`: schema, filesystem, initialization, generation, or projection failure.
- `2`: invalid command or arguments.

`sixpack bridge` remains an internal TypeScript SDK transport. It is not a
normal application command.

Repair, cache-maintenance, and interactive database commands remain out of the
public CLI until their behavior is implemented and contract-tested.
