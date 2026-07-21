use std::fmt;

use crate::SchemaError;

/// Validated workspace directory name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceName(String);

impl WorkspaceName {
    pub fn new(name: impl Into<String>) -> Result<Self, SchemaError> {
        let name = name.into();
        validate_workspace_name(&name)?;
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkspaceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Validated table identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableName(String);

impl TableName {
    pub fn new(name: impl Into<String>) -> Result<Self, SchemaError> {
        let name = name.into();
        if is_schema_name(&name) {
            Ok(Self(name))
        } else {
            Err(SchemaError::InvalidTableName(name))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Validated field or lookup identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldName(String);

impl FieldName {
    pub fn new(name: impl Into<String>) -> Result<Self, SchemaError> {
        let name = name.into();
        if name.starts_with('_') {
            return Err(SchemaError::ReservedFieldName(name));
        }
        if is_schema_name(&name) {
            Ok(Self(name))
        } else {
            Err(SchemaError::InvalidFieldName(name))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FieldName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn validate_workspace_name(name: &str) -> Result<(), SchemaError> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(SchemaError::InvalidWorkspaceName(name.to_owned()))
    }
}

pub(crate) fn is_schema_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    !name.contains("__")
        && first.is_ascii_lowercase()
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
