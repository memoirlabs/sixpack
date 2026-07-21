use std::collections::BTreeMap;

use sixpack_core::{Record, SchemaError, Value};
use sixpack_store::AppendResult;

pub(crate) const DEFAULT_PLAN_LIMIT: usize = 100;
pub(crate) const MAX_PLAN_LIMIT: usize = 1_000;

/// Public database errors.
#[derive(Debug)]
pub enum DatabaseError {
    /// Filesystem-level append/parsing error from the local store.
    Io(std::io::Error),
    /// Schema/validation failure.
    Schema(SchemaError),
    /// Internal plan validation or execution failure.
    Plan(PlanError),
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Schema(error) => write!(formatter, "{error}"),
            Self::Plan(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for DatabaseError {}

impl From<std::io::Error> for DatabaseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SchemaError> for DatabaseError {
    fn from(error: SchemaError) -> Self {
        Self::Schema(error)
    }
}

impl From<PlanError> for DatabaseError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

/// Internal plan operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOp {
    Insert,
    Upsert,
    Patch,
    Remove,
    Get,
    Find,
    Scan,
    Count,
}

/// Internal operation envelope shared by generated APIs and runtime entrypoints.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanEnvelope {
    pub op: PlanOp,
    pub table: String,
    pub lookup: Option<String>,
    pub key: BTreeMap<String, Value>,
    pub value: BTreeMap<String, Value>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

impl PlanEnvelope {
    pub fn new(op: PlanOp, table: impl Into<String>) -> Self {
        Self {
            op,
            table: table.into(),
            lookup: None,
            key: BTreeMap::new(),
            value: BTreeMap::new(),
            limit: None,
            cursor: None,
        }
    }

    pub fn with_lookup(mut self, lookup: impl Into<String>) -> Self {
        self.lookup = Some(lookup.into());
        self
    }

    pub fn with_key(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.key.insert(name.into(), value.into());
        self
    }

    pub fn with_value(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.value.insert(name.into(), value.into());
        self
    }

    pub fn with_record_value(mut self, record: Record) -> Self {
        self.value = record.fields().clone();
        self
    }

    pub fn with_values(mut self, values: BTreeMap<String, Value>) -> Self {
        self.value = values;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }
}

/// Paged row result for plan reads.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanPage {
    pub rows: Vec<Record>,
    pub next_cursor: Option<String>,
}

/// Result of executing one internal plan.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    Append(AppendResult),
    Row(Option<Record>),
    Rows(PlanPage),
    Count(usize),
}

/// Declarative request for current state.
pub trait GetRequest {
    type Output;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError>;

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError>;
}

/// Declarative request for a state change.
pub trait WriteRequest {
    type Output;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError>;

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError>;
}

/// Selector for one row through a unique lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct GetOne {
    plan: PlanEnvelope,
}

impl GetOne {
    pub fn new(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Get, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<Option<Record>, DatabaseError> {
        match outcome {
            PlanOutcome::Row(row) => Ok(row),
            _ => Err(PlanError::Invalid("get selector returned non-row outcome".to_owned()).into()),
        }
    }
}

impl GetRequest for GetOne {
    type Output = Option<Record>;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for many rows through a declared lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct GetMany {
    plan: PlanEnvelope,
}

impl GetMany {
    pub fn new(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Find, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value)
                .with_limit(MAX_PLAN_LIMIT),
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.plan.limit = Some(limit);
        self
    }

    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.plan.cursor = Some(cursor.into());
        self
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<Vec<Record>, DatabaseError> {
        match outcome {
            PlanOutcome::Rows(page) => Ok(page.rows),
            _ => {
                Err(PlanError::Invalid("get selector returned non-rows outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetMany {
    type Output = Vec<Record>;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for a page of table rows.
#[derive(Debug, Clone, PartialEq)]
pub struct GetPage {
    plan: PlanEnvelope,
}

impl GetPage {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Scan, table),
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.plan.limit = Some(limit);
        self
    }

    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.plan.cursor = Some(cursor.into());
        self
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<PlanPage, DatabaseError> {
        match outcome {
            PlanOutcome::Rows(page) => Ok(page),
            _ => {
                Err(PlanError::Invalid("get selector returned non-page outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetPage {
    type Output = PlanPage;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError> {
        Self::from_outcome(outcome)
    }
}

/// Selector for a table or lookup count.
#[derive(Debug, Clone, PartialEq)]
pub struct GetCount {
    plan: PlanEnvelope,
}

impl GetCount {
    pub fn table(table: impl Into<String>) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Count, table),
        }
    }

    pub fn lookup(
        table: impl Into<String>,
        lookup: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Count, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, value),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<usize, DatabaseError> {
        match outcome {
            PlanOutcome::Count(count) => Ok(count),
            _ => {
                Err(PlanError::Invalid("get selector returned non-count outcome".to_owned()).into())
            }
        }
    }
}

impl GetRequest for GetCount {
    type Output = usize;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError> {
        Self::from_outcome(outcome)
    }
}

/// Declarative state change.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteChange {
    plan: PlanEnvelope,
}

impl WriteChange {
    pub fn add_record(record: Record) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Insert, record.table()).with_record_value(record),
        }
    }

    pub fn set_record(record: Record) -> Self {
        Self {
            plan: PlanEnvelope::new(PlanOp::Upsert, record.table()).with_record_value(record),
        }
    }

    pub fn edit(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<Value>,
        value: BTreeMap<String, Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Patch, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, key)
                .with_values(value),
        }
    }

    pub fn remove(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<Value>,
    ) -> Self {
        let lookup = lookup.into();
        Self {
            plan: PlanEnvelope::new(PlanOp::Remove, table)
                .with_lookup(lookup.clone())
                .with_key(lookup, key),
        }
    }

    pub fn into_plan(self) -> PlanEnvelope {
        self.plan
    }

    pub fn from_outcome(outcome: PlanOutcome) -> Result<AppendResult, DatabaseError> {
        match outcome {
            PlanOutcome::Append(result) => Ok(result),
            _ => Err(
                PlanError::Invalid("write change returned non-append outcome".to_owned()).into(),
            ),
        }
    }
}

impl WriteRequest for WriteChange {
    type Output = AppendResult;

    fn into_plan(self) -> Result<PlanEnvelope, DatabaseError> {
        Ok(self.into_plan())
    }

    fn from_outcome(outcome: PlanOutcome) -> Result<Self::Output, DatabaseError> {
        Self::from_outcome(outcome)
    }
}

pub mod selector {
    use super::{GetCount, GetMany, GetOne, GetPage, Value};

    pub fn id(table: impl Into<String>, id: impl Into<String>) -> GetOne {
        GetOne::new(table, "id", Value::Id(id.into()))
    }

    pub fn one(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<String>,
    ) -> GetOne {
        GetOne::new(table, lookup, Value::Text(key.into()))
    }

    pub fn many(
        table: impl Into<String>,
        lookup: impl Into<String>,
        key: impl Into<String>,
    ) -> GetMany {
        GetMany::new(table, lookup, Value::Text(key.into()))
    }

    pub fn all(table: impl Into<String>) -> GetPage {
        GetPage::new(table)
    }

    pub fn count(table: impl Into<String>) -> GetCount {
        GetCount::table(table)
    }
}

pub mod change {
    use std::collections::BTreeMap;

    use super::{Record, Value, WriteChange};

    pub fn add(record: Record) -> WriteChange {
        WriteChange::add_record(record)
    }

    pub fn set(record: Record) -> WriteChange {
        WriteChange::set_record(record)
    }

    pub fn edit_id(
        table: impl Into<String>,
        id: impl Into<String>,
        value: BTreeMap<String, Value>,
    ) -> WriteChange {
        WriteChange::edit(table, "id", Value::Id(id.into()), value)
    }

    pub fn remove_id(table: impl Into<String>, id: impl Into<String>) -> WriteChange {
        WriteChange::remove(table, "id", Value::Id(id.into()))
    }
}

/// Plan-level validation and execution errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    Invalid(String),
    NotFound(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "{message}"),
            Self::NotFound(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for PlanError {}
