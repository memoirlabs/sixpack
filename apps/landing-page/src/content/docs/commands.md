---
title: CLI commands
description: Create a Rust starter, initialize local data, and generate typed APIs.
order: 6
---

The command line covers the complete local setup loop.

## Quickstart

Create a runnable Rust starter:

```sh
sixpack create
```

The terminal asks for a path and presents exactly three examples: Notes, Chat
app, and Topcoat Notes. Use `--template notes`, `--template ai`, or `--template
topcoat` for non-interactive scripts. Chat app is a deterministic local echo
endpoint with a live message database. Topcoat creates a server-rendered
Topcoat 0.5 notes page backed by Sixpack; its generated README uses `topcoat
dev`. The npm launcher is not published yet; `bunx sixpack create` is the
intended future packaging shape.

For a schema-first database without a starter app, put `schema.sixpack` in the
current directory and run:

```sh
sixpack init
```

This initializes `./data` and generates `./data/projection.html`.

Use explicit paths when needed:

```sh
sixpack init ./local-data/assistant --schema ./schemas/assistant.sixpack
```

## Generate typed APIs

```sh
sixpack generate rust --out generated/schema.rs
sixpack generate typescript --out generated/schema.ts
```

The schema defaults to `./schema.sixpack`. Omit `--out` to write generated
source to stdout.

## Refresh the projection

```sh
sixpack project
sixpack project ./local-data/assistant
```

The generated HTML is a read-only snapshot of canonical `.6` rows and opens
directly without a server or upload.

## Help and version

```sh
sixpack help
sixpack help create
sixpack help init
sixpack help generate
sixpack help project
sixpack --version
```

Repair, advanced cache maintenance, and an interactive shell are not
implemented. `sixpack bridge` is an internal TypeScript SDK transport, not a
normal application command.
