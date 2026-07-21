use super::*;

impl LocalStore {
    /// Appends a put event to the `.6` table segment.
    pub fn append_put(&self, schema: &DatabaseSchema, record: &Record) -> io::Result<AppendResult> {
        self.append(schema, Operation::Put, record)
    }

    /// Appends a put only when the id is not already live.
    pub fn append_insert(
        &self,
        schema: &DatabaseSchema,
        record: &Record,
    ) -> io::Result<AppendResult> {
        let batch = WriteBatch::insert_only(record.table(), [record.clone()])?;
        one_append_result(self.append_batch(schema, &batch)?)
    }

    /// Appends multiple puts to one table only when every id is new.
    pub fn append_insert_many(
        &self,
        schema: &DatabaseSchema,
        records: &[Record],
    ) -> io::Result<Vec<AppendResult>> {
        let Some(first) = records.first() else {
            return Ok(Vec::new());
        };
        let batch = WriteBatch::insert_only(first.table(), records.iter().cloned())?;
        self.append_batch(schema, &batch)
    }

    /// Appends multiple operations to one table in one `.6` chunk.
    pub fn append_many(
        &self,
        schema: &DatabaseSchema,
        operations: &[AppendOperation],
    ) -> io::Result<Vec<AppendResult>> {
        let Some(first) = operations.first() else {
            return Ok(Vec::new());
        };
        let batch = WriteBatch::upsert(first.record.table(), operations.iter().cloned())?;
        self.append_batch(schema, &batch)
    }

    /// Appends a prepared write batch to one `.6` chunk.
    pub fn append_batch(
        &self,
        schema: &DatabaseSchema,
        batch: &WriteBatch,
    ) -> io::Result<Vec<AppendResult>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        self.resolve_and_append(schema, batch.table(), |_| Ok(batch.clone()))
    }

    /// Resolves and appends one batch while holding a single write snapshot.
    pub fn resolve_and_append<F, E>(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        resolver: F,
    ) -> Result<Vec<AppendResult>, E>
    where
        F: FnOnce(&WriteSnapshot<'_>) -> Result<WriteBatch, E>,
        E: From<io::Error>,
    {
        let _guard = self.workspace_write_guard().map_err(E::from)?;
        self.refresh_if_revision_changed().map_err(E::from)?;
        self.activate_schema(schema).map_err(E::from)?;
        let recovering_dirty_workspace = self.current_revision().map_err(E::from)? == u64::MAX;
        self.prepare_workspace_for_write(schema, table_name, recovering_dirty_workspace)
            .map_err(E::from)?;
        let snapshot = WriteSnapshot {
            store: self,
            schema,
        };
        let batch = resolver(&snapshot)?;
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        if batch.table() != table_name {
            return Err(E::from(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resolved write batch changed tables",
            )));
        }
        let table = schema
            .table(table_name)
            .ok_or_else(|| E::from(io::Error::new(io::ErrorKind::InvalidInput, "unknown table")))?;
        for operation in batch.operations() {
            validate_storage_operation(schema, table, operation).map_err(E::from)?;
        }
        self.publish_revision(u64::MAX).map_err(E::from)?;
        let results = match self.append_batch_inner(schema, &batch) {
            Ok(results) => results,
            Err(error) => {
                self.invalidate_cached_state().map_err(E::from)?;
                return Err(E::from(error));
            }
        };
        if let Some(last) = results.last() {
            self.publish_revision(last.tx_id.saturating_add(1))
                .map_err(E::from)?;
        }
        Ok(results)
    }

    /// Appends a delete event to the `.6` table segment.
    pub fn append_delete(
        &self,
        schema: &DatabaseSchema,
        record: &Record,
    ) -> io::Result<AppendResult> {
        self.append(schema, Operation::Delete, record)
    }

    /// Appends a delete tombstone by id.
    pub fn append_delete_id(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        id: &str,
    ) -> io::Result<AppendResult> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let mut record = Record::new(table.name());
        record.insert_id(id.to_owned());
        self.append(schema, Operation::Delete, &record)
    }

    /// Appends an operation for a typed record.
    pub fn append(
        &self,
        schema: &DatabaseSchema,
        operation: Operation,
        record: &Record,
    ) -> io::Result<AppendResult> {
        let batch = WriteBatch::upsert(
            record.table(),
            [AppendOperation::new(operation, record.clone())],
        )?;
        one_append_result(self.append_batch(schema, &batch)?)
    }
}
