# Topcoat Notes

Topcoat Notes is an official Sixpack example: one server-rendered notes page
compiled with [Topcoat](https://github.com/tokio-rs/topcoat) and persisted to a
local Sixpack database.

The `sixpack create --template topcoat` starter is generated directly from this
project, so the checked example and generated application cannot drift apart.
The example includes:

- a Topcoat `view!` document, layout, and reusable note component;
- discovered `GET` and `POST` routes;
- progressively enhanced create and delete forms;
- a schema-backed Sixpack notes table;
- local `.6` data, generated `.6b` indexes, and a read-only HTML projection;
- focused persistence and projection-failure tests.

## Run with Topcoat

Install the matching early-stage Topcoat CLI release:

```sh
cargo install topcoat-cli --version 0.5.0
```

Then compile and serve the example through Topcoat:

```sh
cd packages/sixpack/examples/topcoat-notes
SIXPACK_DATABASE=../../../../target/examples/topcoat-notes topcoat dev
```

Open <http://127.0.0.1:3000/>. Set `PORT` when that port is occupied:

```sh
PORT=3046 \
SIXPACK_DATABASE=../../../../target/examples/topcoat-notes \
topcoat dev
```

For a one-off Cargo run from the repository root:

```sh
SIXPACK_DATABASE=target/examples/topcoat-notes \
  cargo run -p topcoat-notes
```

## Generate a new project

After installing Sixpack's CLI:

```sh
sixpack create ./topcoat-sixpack --template topcoat
cd topcoat-sixpack
topcoat dev
```

The generated project owns its copied source and can be changed independently.
