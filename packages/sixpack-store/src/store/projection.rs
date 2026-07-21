use super::*;

impl LocalStore {
    /// Rebuilds the `.6b` cache from canonical `.6` files.
    pub fn rebuild_sixb(&self, schema: &DatabaseSchema, table_name: &str) -> io::Result<SixbCache> {
        let _guard = self.workspace_write_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.rebuild_sixb_inner(schema, table_name)
    }

    pub(super) fn rebuild_sixb_inner(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<SixbCache> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let schema_hash = schema.schema_hash();
        let scan = self.scan_table_files(table, &schema_hash)?;
        let mut rows = Vec::new();
        let mut lookups = Vec::new();

        for (id, live) in &scan.live {
            rows.push(SixbRowEntry {
                id: id.clone(),
                ptr: live.ptr.clone(),
            });
            for lookup in table.lookup_specs_with_implicit_id() {
                let value = live
                    .record
                    .fields()
                    .get(lookup.field_name())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("missing lookup field `{}`", lookup.field_name()),
                        )
                    })?;
                lookups.push(SixbLookupEntry {
                    field_name: lookup.field_name().to_owned(),
                    key: value_to_lookup_key(value),
                    id: id.clone(),
                });
            }
        }
        validate_unique_lookups(table, &lookups)?;
        sort_sixb_entries(&mut rows, &mut lookups);

        let cache = SixbCache {
            version: SIXB_BINARY_VERSION,
            table: table.name().to_owned(),
            schema_hash,
            source_hash: scan.source_hash.clone(),
            rows,
            lookups,
        };
        self.remember_table_records(table.name(), scan.live.iter())?;
        self.write_sixb_cache(table.name(), &cache)?;
        self.remember_runtime_sixb_cache(RuntimeSixb::from_cache(cache.clone()))?;
        Ok(cache)
    }

    /// Loads `.6b` if its header matches the current schema, otherwise rebuilds it from `.6`.
    pub fn ensure_sixb_current(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<SixbCache> {
        let _guard = self.workspace_read_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.ensure_sixb_snapshot(schema, table_name)
            .map(|cache| cache.as_ref().clone())
    }

    /// Rewrites one table to a single canonical `.6` chunk containing current live rows.
    #[cfg(feature = "experimental-compaction")]
    pub fn compact_table(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<CompactionResult> {
        let _workspace_guard = self.workspace_write_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.ensure_workspace_layout()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        self.ensure_table_layout(table, &schema.schema_hash())?;

        let table_dir = self.table_dir(table.name());
        fs::create_dir_all(&table_dir)?;
        let old_paths = six_files_in_read_order(&table_dir)?;
        let chunks_before = old_paths.len();
        let bytes_before = old_paths.iter().try_fold(0u64, |total, path| {
            fs::metadata(path).map(|metadata| total.saturating_add(metadata.len()))
        })?;
        let scan = self.scan_table_files(table, &schema.schema_hash())?;
        let tx_start = self.discovered_next_tx_id()?;

        let mut compacted = encode_six_preamble(table, &schema.schema_hash()).into_bytes();
        for (index, live) in scan.live.values().enumerate() {
            let tx_id = tx_start + index as u64;
            let line = encode_six_operation(table, Operation::Put, tx_id, &live.record)
                .map_err(format_error_to_io)?;
            compacted.extend_from_slice(line.as_bytes());
            compacted.push(b'\n');
        }

        let tmp = table_dir.join("compact.tmp");
        fs::write(&tmp, &compacted)?;
        for path in old_paths {
            fs::remove_file(path)?;
        }

        let chunk_relative_path = chunk_path(0)?;
        let chunk_name = chunk_path_to_name(&chunk_relative_path)?;
        let final_path = table_dir.join(&chunk_relative_path);
        fs::rename(&tmp, &final_path)?;
        let bytes_after = fs::metadata(&final_path)?.len();
        let live_rows = scan.live.len();

        self.forget_table_chunks(table.name())?;
        self.remember_chunk(table.name(), &chunk_name, compacted)?;
        self.forget_sixb_cache(table.name())?;
        self.forget_runtime_sixb_cache(table.name())?;
        self.set_next_chunk_counter(table.name(), 1)?;
        let next_tx = tx_start + live_rows as u64;
        self.set_next_tx_id(next_tx)?;
        self.rebuild_sixb_inner(schema, table.name())?;
        self.write_metadata(schema, next_tx)?;
        self.publish_revision(next_tx)?;

        Ok(CompactionResult {
            table: table.name().to_owned(),
            live_rows,
            chunks_before,
            chunks_after: 1,
            bytes_before,
            bytes_after,
            chunk_name,
        })
    }

    pub(super) fn ensure_sixb_snapshot(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<Arc<SixbCache>> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        let schema_hash = schema.schema_hash();
        let path = self.sixb_path(table.name());
        if let Some(cache) = self.runtime_sixb_to_cache(table.name(), &schema_hash)? {
            return self.remember_sixb_cache(cache);
        }
        if let Some(cache) = self.cached_sixb(table.name(), &schema_hash)? {
            return Ok(cache);
        }
        if path.exists() {
            let bytes = fs::read(&path)?;
            if let Ok(cache) = decode_sixb_cache(&bytes)
                && cache.version == SIXB_BINARY_VERSION
                && cache.table == table.name()
                && cache.schema_hash == schema_hash
                && cache.source_hash == self.scan_table_source_hash(table, &schema_hash)?
            {
                return self.remember_sixb_cache(cache);
            }
        }
        self.rebuild_sixb_inner(schema, table_name)
            .and_then(|cache| self.remember_sixb_cache(cache))
    }

    pub(super) fn ensure_table_layout(
        &self,
        table: &TableSchema,
        schema_hash: &str,
    ) -> io::Result<()> {
        let layout_key = (table.name().to_owned(), table.signature());
        if self
            .layout_cache
            .read()
            .map_err(|_| io::Error::other("layout cache lock poisoned"))?
            .contains(&layout_key)
        {
            return Ok(());
        }
        fs::create_dir_all(self.table_dir(table.name()))?;
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            verify_header(table, schema_hash, &path)?;
        }
        self.layout_cache
            .write()
            .map_err(|_| io::Error::other("layout cache lock poisoned"))?
            .insert(layout_key);
        Ok(())
    }
}
