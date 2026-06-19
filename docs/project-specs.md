# Tensack specs and implementation references

This is the consolidated map of public design/specification documents and how they relate to the current root workspace.

## Current root workspace (active)

- [README.md](../README.md) — current scope, package layout, and build assumptions.
- [TENSACK_BOOK.md](../TENSACK_BOOK.md) — compact source of truth for product/backend decisions.
- [TENSACK_SCHEMA_SPEC.md](../TENSACK_SCHEMA_SPEC.md) — schema authoring, validation, primitive types, and generated metadata.
- [TENSACK_API_SPEC.md](../TENSACK_API_SPEC.md) — target generated API surface.
- [TENSACK_PLAN_SPEC.md](../TENSACK_PLAN_SPEC.md) — internal operation envelope shared by runtime, CLI, admin UI, and SDKs.
- [TENSACK_STORAGE_SPEC.md](../TENSACK_STORAGE_SPEC.md) — local file layout, chunk naming, `.ten`, `.tenb`, and `tensack.toml`.
- [TENSACK_IMPLEMENTATION_STATUS.md](../TENSACK_IMPLEMENTATION_STATUS.md) — implemented behavior versus planned behavior.
- [AGENTS.md](../AGENTS.md) — working constraints for this repository and the source-of-truth model for implementation.
- [SCHEMA_COMPILER.md](../SCHEMA_COMPILER.md) — current schema compiler behavior and integration notes.
- [docs/commands.md](commands.md) — CLI contract stub (currently only `--version`, `help`).
- [docs/file-format.md](file-format.md) — file format scope stub.
- [DATABASE_TESTING.md](../DATABASE_TESTING.md) — testing model and isolation rules.
- [tests/contracts/README.md](../tests/contracts/README.md) — contract test boundary intent.
- [tests/snapshots/README.md](../tests/snapshots/README.md) — snapshot testing intent.
- [benchmark/README.md](../benchmark/README.md) — benchmark intent.
- [apps/landing-page/index.html](../apps/landing-page/index.html) — static docs app for the current backend map and storage layout.
- [apps/admin-ui/README.md](../apps/admin-ui/README.md) — admin UI intent.
- [apps/test-lab/README.md](../apps/test-lab/README.md) — experimental test workspace for speed/sync checks, fixtures, and UI experiments.
- [packages/tensack-testkit/src/lib.rs](../packages/tensack-testkit/src/lib.rs) — shared test helper placeholder in Rust.
- [packages/tensack-schema-compiler/src/lib.rs](../packages/tensack-schema-compiler/src/lib.rs) — build-time schema parser/validator/output.

## Design and architecture specs

- [tensack_rust_backend_architecture.md](../tensack_rust_backend_architecture.md) — older long-form architecture background.
- [tensack_functional_addendum.md](../tensack_functional_addendum.md) — older functional interface background.
- [tensack_chunk_naming_spec.md](../tensack_chunk_naming_spec.md) — detailed chunk naming rationale behind the current storage spec.

## Duplicate / overlap notes

- `TENSACK_BOOK.md` and the focused `TENSACK_*_SPEC.md` files are the current decision docs.
- `tensack_rust_backend_architecture.md` and `tensack_functional_addendum.md` are background references and may contain older examples.

## Direct comparison: current vs spec

- Implemented status today is tracked in [TENSACK_IMPLEMENTATION_STATUS.md](../TENSACK_IMPLEMENTATION_STATUS.md).
- Target generated API direction is tracked in [TENSACK_API_SPEC.md](../TENSACK_API_SPEC.md).
- Internal operation envelope direction is tracked in [TENSACK_PLAN_SPEC.md](../TENSACK_PLAN_SPEC.md).
