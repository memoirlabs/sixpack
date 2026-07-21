use super::*;

impl LocalStore {
    /// Reads all current live records from a table using the generated `.6b` cache.
    pub fn read_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<Vec<Record>> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.read_table_inner(schema, table_name)
    }

    pub(super) fn read_table_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entries) = self.runtime_row_entries(table_name)? {
            let mut rows = Vec::with_capacity(entries.len());
            for entry in &entries {
                rows.push(self.read_row_entry(table, entry)?);
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::with_capacity(cache.rows.len());
        for entry in &cache.rows {
            rows.push(self.read_row_entry(table, entry)?);
        }
        Ok(rows)
    }

    /// Reads one row by implicit id lookup.
    pub fn get_by_id(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        id: &str,
    ) -> io::Result<Option<Record>> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.get_by_id_inner(schema, table_name, id)
    }

    pub(super) fn get_by_id_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        id: &str,
    ) -> io::Result<Option<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entry) = self.runtime_row_entry(table_name, id)? {
            return self.read_row_entry(table, &entry).map(Some);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let Some(entry) = row_entry_by_id(&cache, id) else {
            return Ok(None);
        };
        self.read_row_entry(table, entry).map(Some)
    }

    /// Reads rows by a declared lookup field. Unique lookup callers should use the first item.
    pub fn get_by_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Vec<Record>> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.get_by_lookup_inner(schema, table_name, field_name, key)
    }

    /// Reads one lookup page and its total from the same store snapshot.
    pub fn lookup_page(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
        limit: usize,
        offset: usize,
    ) -> io::Result<ReadPage> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        let rows = self.get_by_lookup_inner(schema, table_name, field_name, key)?;
        let total = rows.len();
        Ok(ReadPage {
            rows: rows.into_iter().skip(offset).take(limit).collect(),
            total,
        })
    }

    pub(super) fn get_by_lookup_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        if field_name == "id" {
            return self
                .get_by_id_inner(schema, table_name, key)
                .map(|row| row.into_iter().collect());
        }
        if let Some(entries) = self.runtime_lookup_entries(table_name, field_name, key)? {
            let mut rows = Vec::with_capacity(entries.len());
            for lookup_entry in entries {
                if let Some(row_entry) = self.runtime_row_entry(table_name, &lookup_entry.id)? {
                    rows.push(self.read_row_entry(table, &row_entry)?);
                }
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::new();
        for lookup_entry in lookup_entries_by_key(&cache, field_name, key) {
            if let Some(row_entry) = row_entry_by_id(&cache, &lookup_entry.id) {
                rows.push(self.read_row_entry(table, row_entry)?);
            }
        }
        Ok(rows)
    }

    /// Reads one row by a unique lookup field.
    pub fn get_unique_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Record>> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.get_unique_lookup_inner(schema, table_name, field_name, key)
    }

    pub(super) fn get_unique_lookup_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name == "id" {
            return self.get_by_id_inner(schema, table_name, key);
        }
        let lookup = table.lookup(field_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            )
        })?;
        if !lookup.unique() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("lookup `{field_name}` for table `{table_name}` is not unique"),
            ));
        }
        let rows = self.get_by_lookup_inner(schema, table_name, field_name, key)?;
        Ok(rows.into_iter().next())
    }

    /// Reads a page of live rows from a table.
    pub fn scan_table(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        limit: usize,
        offset: usize,
    ) -> io::Result<Vec<Record>> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.scan_table_inner(schema, table_name, limit, offset)
    }

    pub(super) fn scan_table_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        limit: usize,
        offset: usize,
    ) -> io::Result<Vec<Record>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(entries) = self.runtime_row_entries(table_name)? {
            let mut rows = Vec::new();
            for entry in entries.iter().skip(offset).take(limit) {
                rows.push(self.read_row_entry(table, entry)?);
            }
            return Ok(rows);
        }

        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        let mut rows = Vec::new();
        for entry in cache.rows.iter().skip(offset).take(limit) {
            rows.push(self.read_row_entry(table, entry)?);
        }
        Ok(rows)
    }

    /// Reads one table page and its total from the same store snapshot.
    pub fn scan_page(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        limit: usize,
        offset: usize,
    ) -> io::Result<ReadPage> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        let total = self.count_table_inner(schema, table_name)?;
        let rows = self.scan_table_inner(schema, table_name, limit, offset)?;
        Ok(ReadPage { rows, total })
    }

    /// Counts current live rows in one table.
    pub fn count_table(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<usize> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.count_table_inner(schema, table_name)
    }

    pub(super) fn count_table_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<usize> {
        schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if let Some(count) = self.runtime_row_count(table_name)? {
            return Ok(count);
        }
        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        Ok(cache.rows.len())
    }

    /// Counts current live rows matching a lookup key.
    pub fn count_lookup(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<usize> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        if field_name != "id" && table.lookup(field_name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown lookup `{field_name}` for table `{table_name}`"),
            ));
        }
        if field_name == "id" {
            if let Some(entry) = self.runtime_row_entry(table_name, key)? {
                return Ok(usize::from(entry.id == key));
            }
            let cache = self.ensure_sixb_snapshot(schema, table_name)?;
            return Ok(usize::from(row_entry_by_id(&cache, key).is_some()));
        }
        if let Some(count) = self.runtime_lookup_count(table_name, field_name, key)? {
            return Ok(count);
        }
        let cache = self.ensure_sixb_snapshot(schema, table_name)?;
        Ok(lookup_entries_by_key(&cache, field_name, key).len())
    }
}
