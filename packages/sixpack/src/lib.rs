//! Public sixpack database API.
//!
//! Applications should normally use `Database::open_path_with_schema`,
//! generated selectors, and generated changes. `DatabaseOptions` remains
//! available when an application already models root and workspace separately.

mod database;
mod macros;
mod options;
mod projection;
mod request;

pub use database::*;
pub use options::DatabaseOptions;
pub use projection::{DataProjection, write_data_projection};
pub use request::*;

/// Stable plumbing used by generated Rust APIs.
pub mod runtime {
    pub use crate::{
        DatabaseError, GetRequest, PlanEnvelope, PlanError, PlanOp, PlanOutcome, PlanPage,
        WriteRequest,
    };
}

/// Low-level compatibility surface. Normal applications should use `Database`
/// and generated selectors/changes instead.
pub mod raw {
    pub use sixpack_core::{DatabaseSchema, Record, TableSchema, Value};
    pub use sixpack_format::Operation;
    pub use sixpack_store::{
        AppendOperation, AppendResult, GeneratedIndexKind, LocalStore, ReadPage, WriteBatch,
        WriteBatchMode, WriteSnapshot,
    };
}
