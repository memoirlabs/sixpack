#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    name: String,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn try_new(name: impl Into<String>) -> Result<Self, crate::SchemaError> {
        let name = crate::WorkspaceName::new(name)?;
        Ok(Self::new(name.as_str()))
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
