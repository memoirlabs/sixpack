---
title: CLI commands
description: The intentionally small command surface available in v0.0.1.
order: 6
---

The command line remains narrow while the storage engine and generated API settle. Only stable behavior that exists in the binary is documented as implemented.

## Help

```sh
sixpack help
sixpack --help
sixpack -h
```

These forms print the current usage text.

## Version

```sh
sixpack --version
sixpack -V
```

These forms print the CLI version.

## Not implemented yet

The binary does not currently initialize a database, inspect storage, rebuild indexes, repair files, or run an interactive shell. Those commands remain planned and should not be used in application instructions until their code and contract tests exist.

Applications use the Rust `sixpack` crate and generated schema API for current database behavior.
