# Topcoat Notes

A single-page notes example rendered by
[Topcoat](https://github.com/tokio-rs/topcoat) and persisted to a local
Sixpack database.

This package includes the same Rust source as the `sixpack create --template
topcoat` starter, so workspace checks compile the template against the pinned
Topcoat release.

Install Topcoat's CLI and run its development compiler:

```sh
cargo install topcoat-cli --version 0.5.0
cd apps/test-lab/topcoat-notes
SIXPACK_DATABASE=../../../target/test-lab/topcoat-notes topcoat dev
```

Open <http://127.0.0.1:3000/>. Topcoat compiles the `view!` page and routes,
while Sixpack stores notes under the configured database path.

For a one-off run without the live-reload compiler:

```sh
SIXPACK_DATABASE=target/test-lab/topcoat-notes \
  cargo run -p topcoat-notes
```
