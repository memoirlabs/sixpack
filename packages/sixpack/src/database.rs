//! Public sixpack database API.
//!
//! This crate composes the core data model, file format boundary, and local
//! storage engine. Apps should usually depend on this crate instead of wiring
//! lower-level packages together directly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::DatabaseOptions;
pub use sixpack_core::{
    DatabaseSchema, FieldName, FieldSpec, PrimitiveType, Record, SchemaError, TableName,
    TableSchema, Value, Workspace, WorkspaceName,
};
pub use sixpack_format::Operation;
#[cfg(feature = "experimental-compaction")]
pub use sixpack_store::CompactionResult;
use sixpack_store::WriteSnapshot;
pub use sixpack_store::{AppendOperation, AppendResult, LocalStore, WriteBatch, WriteBatchMode};

use crate::request::*;
use crate::request::{DEFAULT_PLAN_LIMIT, MAX_PLAN_LIMIT};

/// A local sixpack database handle.
#[derive(Debug, Clone, PartialEq)]
pub struct Database {
    workspace: Workspace,
    store: LocalStore,
    schema: DatabaseSchema,
}

impl Database {
    /// Opens a database from validated options.
    pub fn open(options: DatabaseOptions) -> Self {
        let (root, workspace_name, schema) = options.into_parts();
        let workspace = Workspace::new(workspace_name.as_str());
        let store = LocalStore::new(root, workspace_name.as_str());
        Self {
            workspace,
            store,
            schema,
        }
    }

    /// Opens a local database handle with an empty schema.
    pub fn open_local(root: impl Into<PathBuf>, workspace_name: impl Into<String>) -> Self {
        Self::open_local_with_schema(root, workspace_name, DatabaseSchema::new())
    }

    /// Opens a local database handle bound to a schema.
    pub fn open_local_with_schema(
        root: impl Into<PathBuf>,
        workspace_name: impl Into<String>,
        schema: DatabaseSchema,
    ) -> Self {
        let workspace_name = workspace_name.into();
        let workspace = Workspace::new(&workspace_name);
        let store = LocalStore::new(root, workspace_name);

        Self {
            workspace,
            store,
            schema,
        }
    }

    /// Returns the workspace.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns the local store root.
    pub fn store_root(&self) -> &Path {
        self.store.root()
    }

    /// Returns configured schema.
    pub fn schema(&self) -> &DatabaseSchema {
        &self.schema
    }

    /// Replaces schema for this database handle.
    pub fn with_schema(mut self, schema: DatabaseSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Creates the empty database layout for all tables in the current schema.
    pub fn init(&self) -> Result<(), DatabaseError> {
        self.store.init(&self.schema).map_err(DatabaseError::from)
    }

    /// Gets the current state for one declarative selector.
    pub fn get<R: GetRequest>(&self, request: R) -> Result<R::Output, DatabaseError> {
        let outcome = self.execute_plan(request.into_plan()?)?;
        R::from_outcome(outcome)
    }

    /// Applies one declarative state change.
    pub fn write<W: WriteRequest>(&self, request: W) -> Result<W::Output, DatabaseError> {
        let outcome = self.execute_plan(request.into_plan()?)?;
        W::from_outcome(outcome)
    }

    /// Applies multiple state changes for one table in one storage batch.
    pub fn write_many(&self, changes: &[WriteChange]) -> Result<Vec<AppendResult>, DatabaseError> {
        let plans = changes
            .iter()
            .cloned()
            .map(WriteChange::into_plan)
            .collect::<Vec<_>>();
        let Some(first) = plans.first() else {
            return Ok(Vec::new());
        };
        if plans.iter().any(|plan| plan.table != first.table) {
            return Err(PlanError::Invalid(
                "write_many changes must all belong to the same table".to_owned(),
            )
            .into());
        }
        if plans.iter().all(|plan| plan.op == PlanOp::Insert) {
            let records = plans
                .iter()
                .map(|plan| {
                    let table = self.schema.table(&plan.table).ok_or_else(|| {
                        PlanError::Invalid(format!("unknown table `{}`", plan.table))
                    })?;
                    let record = self.record_from_plan_value(table.name(), &plan.value)?;
                    self.schema.validate_record(&record)?;
                    Ok(record)
                })
                .collect::<Result<Vec<_>, DatabaseError>>()?;
            return self.insert_many(&records);
        }
        if plans.iter().any(|plan| plan.op == PlanOp::Insert) {
            return Err(PlanError::Invalid(
                "write_many insert changes cannot be mixed with other changes; use insert_many"
                    .to_owned(),
            )
            .into());
        }

        let table_name = first.table.clone();
        self.store
            .resolve_and_append(&self.schema, &table_name, |snapshot| {
                let mut batch = WriteBatch::new(&table_name, WriteBatchMode::Upsert);
                let mut touched_ids = BTreeSet::new();
                for plan in &plans {
                    let table = self.schema.table(&plan.table).ok_or_else(|| {
                        PlanError::Invalid(format!("unknown table `{}`", plan.table))
                    })?;
                    match plan.op {
                        PlanOp::Upsert => {
                            let record = self.record_from_plan_value(table.name(), &plan.value)?;
                            self.schema.validate_record(&record)?;
                            let id = record_id(&record)?;
                            ensure_untouched_id(&mut touched_ids, &id)?;
                            batch.push(AppendOperation::put(record))?;
                        }
                        PlanOp::Patch => {
                            validate_patch_plan(table, plan)?;
                            let mut row = self.require_unique_row_in(snapshot, plan)?;
                            let id = record_id(&row)?;
                            ensure_untouched_id(&mut touched_ids, &id)?;
                            for (name, value) in &plan.value {
                                row.insert_field(name, value.clone())?;
                            }
                            self.schema.validate_record(&row)?;
                            batch.push(AppendOperation::put(row))?;
                        }
                        PlanOp::Remove => {
                            let row = self.require_unique_row_in(snapshot, plan)?;
                            let id = record_id(&row)?;
                            ensure_untouched_id(&mut touched_ids, &id)?;
                            let mut record = Record::new(table.name());
                            record.insert_id(id);
                            batch.push(AppendOperation::delete(record))?;
                        }
                        PlanOp::Get
                        | PlanOp::Find
                        | PlanOp::Scan
                        | PlanOp::Count
                        | PlanOp::Insert => {
                            return Err(PlanError::Invalid(
                                "write_many only accepts state changes".to_owned(),
                            )
                            .into());
                        }
                    }
                }
                Ok(batch)
            })
    }

    /// Inserts a new row. Fails if the id already exists.
    pub fn insert(&self, record: &Record) -> Result<AppendResult, DatabaseError> {
        self.write(WriteChange::add_record(record.clone()))
    }

    /// Inserts multiple new rows for one table in one storage batch.
    pub fn insert_many(&self, records: &[Record]) -> Result<Vec<AppendResult>, DatabaseError> {
        let Some(first) = records.first() else {
            return Ok(Vec::new());
        };
        for record in records {
            if record.table() != first.table() {
                return Err(PlanError::Invalid(
                    "insert_many records must all belong to the same table".to_owned(),
                )
                .into());
            }
            self.schema.validate_record(record)?;
        }
        self.store
            .append_insert_many(&self.schema, records)
            .map_err(DatabaseError::from)
    }

    /// Writes a replacement row, or inserts it if it does not exist.
    pub fn put(&self, record: &Record) -> Result<AppendResult, DatabaseError> {
        self.upsert(record)
    }

    /// Writes a replacement row, or inserts it if it does not exist.
    pub fn upsert(&self, record: &Record) -> Result<AppendResult, DatabaseError> {
        self.write(WriteChange::set_record(record.clone()))
    }

    /// Writes a delete operation using the id from a row-like record.
    pub fn delete(&self, record: &Record) -> Result<AppendResult, DatabaseError> {
        let id = record_id(record)?;
        self.delete_by_id(record.table(), &id)
    }

    /// Deletes a row by id. This does not require the rest of the row fields.
    pub fn delete_by_id(&self, table_name: &str, id: &str) -> Result<AppendResult, DatabaseError> {
        self.store
            .append_delete_id(&self.schema, table_name, id)
            .map_err(DatabaseError::from)
    }

    /// Writes a row with a specific operation (advanced path).
    pub fn apply(
        &self,
        operation: Operation,
        record: &Record,
    ) -> Result<AppendResult, DatabaseError> {
        self.schema.validate_record(record)?;
        self.store
            .append(&self.schema, operation, record)
            .map_err(DatabaseError::from)
    }

    /// Compatibility path for reading the current live row by table id.
    pub fn get_by_id(&self, table_name: &str, id: &str) -> Result<Option<Record>, DatabaseError> {
        self.get(selector::id(table_name, id))
    }

    /// Reads the first current live row matching a lookup key.
    pub fn get_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Option<Record>, DatabaseError> {
        match self.execute_plan(
            PlanEnvelope::new(PlanOp::Get, table_name)
                .with_lookup(lookup_field)
                .with_key(lookup_field, Value::Text(key.to_owned())),
        )? {
            PlanOutcome::Row(row) => Ok(row),
            _ => unreachable!("get plans return row results"),
        }
    }

    /// Reads all current live rows matching a lookup key.
    pub fn get_many_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
    ) -> Result<Vec<Record>, DatabaseError> {
        match self.execute_plan(
            PlanEnvelope::new(PlanOp::Find, table_name)
                .with_lookup(lookup_field)
                .with_key(lookup_field, Value::Text(key.to_owned()))
                .with_limit(MAX_PLAN_LIMIT),
        )? {
            PlanOutcome::Rows(page) => Ok(page.rows),
            _ => unreachable!("find plans return row pages"),
        }
    }

    /// Reads one page of rows matching a declared lookup key.
    pub fn get_page_by(
        &self,
        table_name: &str,
        lookup_field: &str,
        key: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<PlanPage, DatabaseError> {
        let mut plan = PlanEnvelope::new(PlanOp::Find, table_name)
            .with_lookup(lookup_field)
            .with_key(lookup_field, Value::Text(key.to_owned()))
            .with_limit(limit);
        plan.cursor = cursor.map(str::to_owned);
        match self.execute_plan(plan)? {
            PlanOutcome::Rows(page) => Ok(page),
            _ => unreachable!("find plans return row pages"),
        }
    }

    /// Applies a partial update to one row addressed by a unique lookup.
    pub fn patch_by_id(
        &self,
        table_name: &str,
        id: &str,
        patch: BTreeMap<String, Value>,
    ) -> Result<AppendResult, DatabaseError> {
        match self.execute_plan(PlanEnvelope {
            op: PlanOp::Patch,
            table: table_name.to_owned(),
            lookup: Some("id".to_owned()),
            key: BTreeMap::from([("id".to_owned(), Value::Id(id.to_owned()))]),
            value: patch,
            limit: None,
            cursor: None,
        })? {
            PlanOutcome::Append(result) => Ok(result),
            _ => unreachable!("patch plans return append results"),
        }
    }

    /// Reads live rows from a table without a lookup.
    pub fn scan(
        &self,
        table_name: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<PlanPage, DatabaseError> {
        let mut plan = PlanEnvelope::new(PlanOp::Scan, table_name);
        plan.limit = limit;
        plan.cursor = cursor.map(str::to_owned);
        match self.execute_plan(plan)? {
            PlanOutcome::Rows(page) => Ok(page),
            _ => unreachable!("scan plans return row pages"),
        }
    }

    /// Counts current live rows in a table.
    pub fn count(&self, table_name: &str) -> Result<usize, DatabaseError> {
        match self.execute_plan(PlanEnvelope::new(PlanOp::Count, table_name))? {
            PlanOutcome::Count(count) => Ok(count),
            _ => unreachable!("count plans return counts"),
        }
    }

    /// Executes one validated internal plan envelope.
    pub fn execute_plan(&self, plan: PlanEnvelope) -> Result<PlanOutcome, DatabaseError> {
        let table = self
            .schema
            .table(&plan.table)
            .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;

        match plan.op {
            PlanOp::Insert => {
                let record = self.record_from_plan_value(table.name(), &plan.value)?;
                self.schema.validate_record(&record)?;
                self.store
                    .append_insert(&self.schema, &record)
                    .map(PlanOutcome::Append)
                    .map_err(DatabaseError::from)
            }
            PlanOp::Upsert => {
                let record = self.record_from_plan_value(table.name(), &plan.value)?;
                self.schema.validate_record(&record)?;
                self.store
                    .append_put(&self.schema, &record)
                    .map(PlanOutcome::Append)
                    .map_err(DatabaseError::from)
            }
            PlanOp::Patch => {
                validate_patch_plan(table, &plan)?;
                let table_name = table.name().to_owned();
                let results =
                    self.store
                        .resolve_and_append(&self.schema, &table_name, |snapshot| {
                            let mut row = self.require_unique_row_in(snapshot, &plan)?;
                            for (name, value) in &plan.value {
                                row.insert_field(name, value.clone())?;
                            }
                            self.schema.validate_record(&row)?;
                            WriteBatch::upsert(&table_name, [AppendOperation::put(row)])
                                .map_err(DatabaseError::from)
                        })?;
                one_database_append_result(results).map(PlanOutcome::Append)
            }
            PlanOp::Remove => {
                let table_name = table.name().to_owned();
                let results =
                    self.store
                        .resolve_and_append(&self.schema, &table_name, |snapshot| {
                            let row = self.require_unique_row_in(snapshot, &plan)?;
                            let id = record_id(&row)?;
                            let mut record = Record::new(&table_name);
                            record.insert_id(id);
                            WriteBatch::upsert(&table_name, [AppendOperation::delete(record)])
                                .map_err(DatabaseError::from)
                        })?;
                one_database_append_result(results).map(PlanOutcome::Append)
            }
            PlanOp::Get => self.optional_unique_row(&plan).map(PlanOutcome::Row),
            PlanOp::Find => {
                let lookup = self.require_lookup_name(&plan)?;
                self.require_declared_lookup(table, &lookup)?;
                let key = self.require_lookup_key(&plan, &lookup)?;
                let limit = checked_limit(plan.limit)?;
                let cursor = checked_cursor(plan.cursor.as_deref())?;
                let page = self.store.lookup_page(
                    &self.schema,
                    table.name(),
                    &lookup,
                    &key,
                    limit,
                    cursor,
                )?;
                let next_cursor = next_cursor(cursor, limit, page.total);
                Ok(PlanOutcome::Rows(PlanPage {
                    rows: page.rows,
                    next_cursor,
                }))
            }
            PlanOp::Scan => {
                let limit = checked_limit(plan.limit)?;
                let cursor = checked_cursor(plan.cursor.as_deref())?;
                let page = self
                    .store
                    .scan_page(&self.schema, table.name(), limit, cursor)?;
                let next_cursor = next_cursor(cursor, limit, page.total);
                Ok(PlanOutcome::Rows(PlanPage {
                    rows: page.rows,
                    next_cursor,
                }))
            }
            PlanOp::Count => {
                if let Some(lookup) = plan.lookup.as_deref() {
                    self.require_declared_lookup(table, lookup)?;
                    let key = self.require_lookup_key(&plan, lookup)?;
                    self.store
                        .count_lookup(&self.schema, table.name(), lookup, &key)
                        .map(PlanOutcome::Count)
                        .map_err(DatabaseError::from)
                } else {
                    self.store
                        .count_table(&self.schema, table.name())
                        .map(PlanOutcome::Count)
                        .map_err(DatabaseError::from)
                }
            }
        }
    }

    /// Rebuilds the generated `.6b` cache for one table from canonical `.6`.
    pub fn rebuild_cache(&self, table_name: &str) -> Result<(), DatabaseError> {
        self.store
            .rebuild_sixb(&self.schema, table_name)
            .map(|_| ())
            .map_err(DatabaseError::from)
    }

    /// Rewrites one table's canonical `.6` data to current live rows only.
    #[cfg(feature = "experimental-compaction")]
    pub fn compact_table(&self, table_name: &str) -> Result<CompactionResult, DatabaseError> {
        self.store
            .compact_table(&self.schema, table_name)
            .map_err(DatabaseError::from)
    }

    fn record_from_plan_value(
        &self,
        table_name: &str,
        value: &BTreeMap<String, Value>,
    ) -> Result<Record, DatabaseError> {
        let mut record = Record::new(table_name);
        for (name, value) in value {
            record.insert_field(name, value.clone())?;
        }
        Ok(record)
    }

    fn optional_unique_row(&self, plan: &PlanEnvelope) -> Result<Option<Record>, DatabaseError> {
        let table = self
            .schema
            .table(&plan.table)
            .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;
        let lookup = self.require_lookup_name(plan)?;
        self.require_unique_lookup(table, &lookup)?;
        let key = self.require_lookup_key(plan, &lookup)?;
        self.store
            .get_unique_lookup(&self.schema, table.name(), &lookup, &key)
            .map_err(DatabaseError::from)
    }

    fn optional_unique_row_in(
        &self,
        snapshot: &WriteSnapshot<'_>,
        plan: &PlanEnvelope,
    ) -> Result<Option<Record>, DatabaseError> {
        let table = self
            .schema
            .table(&plan.table)
            .ok_or_else(|| PlanError::Invalid(format!("unknown table `{}`", plan.table)))?;
        let lookup = self.require_lookup_name(plan)?;
        self.require_unique_lookup(table, &lookup)?;
        let key = self.require_lookup_key(plan, &lookup)?;
        snapshot
            .get_unique_lookup(table.name(), &lookup, &key)
            .map_err(DatabaseError::from)
    }

    fn require_unique_row_in(
        &self,
        snapshot: &WriteSnapshot<'_>,
        plan: &PlanEnvelope,
    ) -> Result<Record, DatabaseError> {
        self.optional_unique_row_in(snapshot, plan)?.ok_or_else(|| {
            PlanError::NotFound(format!(
                "row not found in `{}` for unique lookup `{}`",
                plan.table,
                plan.lookup.as_deref().unwrap_or("<missing>")
            ))
            .into()
        })
    }

    fn require_lookup_name(&self, plan: &PlanEnvelope) -> Result<String, PlanError> {
        plan.lookup
            .clone()
            .ok_or_else(|| PlanError::Invalid("plan missing lookup".to_owned()))
    }

    fn require_lookup_key(&self, plan: &PlanEnvelope, lookup: &str) -> Result<String, PlanError> {
        let value = plan
            .key
            .get(lookup)
            .ok_or_else(|| PlanError::Invalid(format!("plan missing key `{lookup}`")))?;
        Ok(value_to_lookup_key(value))
    }

    fn require_unique_lookup(&self, table: &TableSchema, lookup: &str) -> Result<(), PlanError> {
        if lookup == "id" {
            return Ok(());
        }
        let spec = table.lookup(lookup).ok_or_else(|| {
            PlanError::Invalid(format!(
                "unknown lookup `{lookup}` for table `{}`",
                table.name()
            ))
        })?;
        if !spec.unique() {
            return Err(PlanError::Invalid(format!(
                "lookup `{lookup}` for table `{}` is not unique",
                table.name()
            )));
        }
        Ok(())
    }

    fn require_declared_lookup(&self, table: &TableSchema, lookup: &str) -> Result<(), PlanError> {
        if lookup == "id" || table.lookup(lookup).is_some() {
            return Ok(());
        }
        Err(PlanError::Invalid(format!(
            "unknown lookup `{lookup}` for table `{}`",
            table.name()
        )))
    }
}

fn checked_limit(limit: Option<usize>) -> Result<usize, PlanError> {
    let limit = limit.unwrap_or(DEFAULT_PLAN_LIMIT);
    if limit == 0 || limit > MAX_PLAN_LIMIT {
        return Err(PlanError::Invalid(format!(
            "limit must be between 1 and {MAX_PLAN_LIMIT}"
        )));
    }
    Ok(limit)
}

fn checked_cursor(cursor: Option<&str>) -> Result<usize, PlanError> {
    match cursor {
        Some(value) if !value.is_empty() => value
            .parse::<usize>()
            .map_err(|error| PlanError::Invalid(format!("invalid cursor: {error}"))),
        _ => Ok(0),
    }
}

fn next_cursor(offset: usize, limit: usize, total: usize) -> Option<String> {
    let next = offset.saturating_add(limit);
    (next < total).then(|| next.to_string())
}

fn validate_patch_plan(table: &TableSchema, plan: &PlanEnvelope) -> Result<(), PlanError> {
    if plan.value.is_empty() {
        return Err(PlanError::Invalid("patch value cannot be empty".to_owned()));
    }
    if plan.value.contains_key("id") {
        return Err(PlanError::Invalid("patch cannot change id".to_owned()));
    }
    for field in plan.value.keys() {
        if table.field(field).is_none() {
            return Err(PlanError::Invalid(format!(
                "unknown field `{field}` for table `{}`",
                table.name()
            )));
        }
    }
    Ok(())
}

fn value_to_lookup_key(value: &Value) -> String {
    match value {
        Value::Id(value) | Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

fn record_id(record: &Record) -> Result<String, DatabaseError> {
    match record.fields().get("id") {
        Some(Value::Id(value)) | Some(Value::Text(value)) => Ok(value.clone()),
        Some(value) => Err(DatabaseError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("record id must be id/text, got {}", value.value_type()),
        ))),
        None => Err(DatabaseError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "record missing id",
        ))),
    }
}

fn ensure_untouched_id(touched_ids: &mut BTreeSet<String>, id: &str) -> Result<(), DatabaseError> {
    if touched_ids.insert(id.to_owned()) {
        Ok(())
    } else {
        Err(PlanError::Invalid(format!("write_many touches row `{id}` more than once")).into())
    }
}

fn one_database_append_result(
    mut results: Vec<AppendResult>,
) -> Result<AppendResult, DatabaseError> {
    results.pop().ok_or_else(|| {
        PlanError::Invalid("single write produced no append result".to_owned()).into()
    })
}
