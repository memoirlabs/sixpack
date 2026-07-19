# Schema Compiler

The schema compiler is build-time infrastructure.

It should parse schema input, validate it, and emit generated Rust or
TypeScript. The Rust runtime should not parse schema text as its normal
embedded path. The current short-lived TypeScript bridge reparses the source
schema on each invocation as a deliberately simple first transport; a future
persistent bridge should load compiled schema state once.

## Current Crate

```txt
packages/sixpack-schema-compiler
```

## Current Responsibilities

- parse `schema! { ... }`
- validate table names
- validate field names
- validate primitive types
- validate duplicate tables and fields
- validate lookup targets
- build `SchemaIr`
- convert IR to runtime `DatabaseSchema`
- emit raw Rust generated API code
- emit TypeScript row/table API code

## Current API

```rust
compile_schema(source)
validate_schema(ir)
database_schema_from_ir(ir)
emit_raw_rust(ir)
emit_typescript(ir)
```

## Generated Shape

Generated Rust currently includes:

- typed `Row`
- `Row::into_record`
- `Row::from_record`
- `Patch`
- unique lookup keys
- generated `by` selectors for `db.get(...)`
- generated `all` and `count` selectors
- generated `add`, `set`, `edit`, and `remove` changes for `db.write(...)`
- table extension trait

Generated TypeScript currently includes:

- typed row interfaces
- exact TypeScript `bigint` fields for Rust `i64`
- generated lookup selectors and unique keys
- generated add/set/edit/remove changes
- a schema hash bound to the Rust bridge

## Next Compiler Work

- additional stable snapshots for generated Rust output
- normal build integration path
- final generated API naming pass
- less stringly runtime glue where Rust types can carry the information
