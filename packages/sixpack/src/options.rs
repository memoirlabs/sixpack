use std::path::{Path, PathBuf};

use crate::{DatabaseSchema, SchemaError, WorkspaceName};

/// Validated options for opening a local database.
///
/// `sixpack.toml` remains recoverable engine metadata. Application configuration
/// belongs in this value (or in an application-owned configuration file that
/// constructs it), not in the database directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseOptions {
    root: PathBuf,
    workspace: WorkspaceName,
    schema: DatabaseSchema,
}

impl DatabaseOptions {
    pub fn new(
        root: impl Into<PathBuf>,
        workspace: impl Into<String>,
        schema: DatabaseSchema,
    ) -> Result<Self, SchemaError> {
        Ok(Self {
            root: root.into(),
            workspace: WorkspaceName::new(workspace)?,
            schema,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace(&self) -> &WorkspaceName {
        &self.workspace
    }

    pub fn schema(&self) -> &DatabaseSchema {
        &self.schema
    }

    pub(crate) fn into_parts(self) -> (PathBuf, WorkspaceName, DatabaseSchema) {
        (self.root, self.workspace, self.schema)
    }
}
