//! Local directory-backed storage engine.
//!
//! The store accepts schema-validated storage operations and keeps `.6` as the
//! canonical append log. `.6b` and future `.6x` files are generated indexes.

mod store;

pub use store::*;
