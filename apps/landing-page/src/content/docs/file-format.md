---
title: File format
description: The canonical .6 table files, generated .6b indexes, metadata, and recovery boundaries.
order: 5
---

A sixpack directory separates readable canonical data from generated runtime state.

## Directory layout

```text
my-app.sixpack/
  schema.sixpack
  sixpack.toml
  tables/
    users/
      zzz.6
      zzy.6
    messages/
      zzz.6
  engine/
    workspace.lock
    revision
    users.6b
    messages.6b
```

| Path | Role |
| --- | --- |
| `schema.sixpack` | Logical schema truth. |
| `tables/<table>/*.6` | Canonical append-only row operations. |
| `engine/*.6b` | Generated binary row pointers and lookup indexes. |
| `engine/workspace.lock` | Local reader and writer coordination. |
| `engine/revision` | Dirty or clean commit and cache revision marker. |
| `sixpack.toml` | Compact recoverable engine metadata. |

## `.6` table segments

Each `.6` file starts with table schema information followed by append-only operations. `R` records write a full replacement row. `D` records write a delete tombstone by id.

```text
SIX<TAB>1<TAB>table<TAB>messages<TAB><schema_hash>
@field<TAB>id<TAB>id
@field<TAB>body<TAB>text
@lookup<TAB>id<TAB>unique
@data
R<TAB>1<TAB>m1<TAB>hello
D<TAB>2<TAB>m1
```

Text values escape backslash, tab, newline, and carriage return. Table chunks stay flat under `tables/<table>/` and use reverse lowercase base36 names such as `zzz.6`, `zzy.6`, and `zzx.6`.

## `.6b` generated indexes

A `.6b` file stores the format version, table name, schema hash, source hash, live row pointers, and lookup entries. It is acceleration data, not canonical truth. Missing or stale indexes can be rebuilt from the schema and `.6` files.

## Interrupted writes

`engine/revision` marks a commit in progress. When the workspace is dirty, opening the database can recover an incomplete non-newline tail before the next append. The same malformed tail under a clean revision fails closed and remains untouched.

`sixpack.toml` tracks physical layout and counters. It is neither the schema nor the row index, and fresh handles can recover its counters from canonical table data.

See the [full file-format contract](https://github.com/memoirlabs/sixpack/blob/main/packages/docs/file-format.md) for exact encoding and validation rules.
