mod error;
mod identifier;
mod record;
mod schema;
mod type_declarations;
mod value;
mod workspace;

pub use error::SchemaError;
pub use identifier::{FieldName, TableName, WorkspaceName};
pub use record::Record;
pub use schema::{DatabaseSchema, FieldSpec, LookupSpec, TableSchema};
pub use type_declarations::{
    PRIMITIVE_TYPE_DECLARATIONS, PrimitiveTypeDecl, find_decl, rust_type_name,
};
pub use value::{PrimitiveType, Value};
pub use workspace::Workspace;

#[cfg(test)]
#[path = "../tests/support/unit.rs"]
mod tests;
