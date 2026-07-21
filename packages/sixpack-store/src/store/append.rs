use super::*;

impl LocalStore {
    pub(super) fn append_batch_inner(
        &self,
        schema: &DatabaseSchema,
        batch: &WriteBatch,
    ) -> io::Result<Vec<AppendResult>> {
        if batch.is_empty() {
            return Ok(Vec::new());
        };
        self.ensure_workspace_layout()?;
        let table = schema
            .table(batch.table())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
        self.ensure_table_layout(table, &schema.schema_hash())?;
        let mut cache = self.take_runtime_sixb_for_write(schema, table.name())?;
        let tx_start = self.next_tx_id()?;
        let mut encoded = Vec::with_capacity(batch.operations().len());
        let mut batch_ids = BTreeSet::new();
        let mut batch_unique = BTreeMap::<(String, String), String>::new();
        let mut validation_cache = cache.clone();

        for append in batch.operations() {
            validate_storage_operation(schema, table, append)?;
            let id = record_id(&append.record)?;
            if batch.mode() == WriteBatchMode::InsertOnly && cache.has_row(&id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("row `{}` already exists in `{}`", id, table.name()),
                ));
            }
            validation_cache.remove_row_for_validation(table, &id)?;
        }

        for (index, append) in batch.operations().iter().enumerate() {
            let id = record_id(&append.record)?;
            if !batch_ids.insert(id.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("write batch touches row `{id}` more than once"),
                ));
            }
            if append.operation == Operation::Put {
                validate_put_unique_lookup_conflicts(
                    table,
                    &validation_cache,
                    &append.record,
                    &mut batch_unique,
                )?;
            }

            let tx_id = tx_start + index as u64;
            let line = encode_six_operation(table, append.operation, tx_id, &append.record)
                .map_err(format_error_to_io)?;
            let bytes_written = (line.len() + 1) as u64;
            encoded.push(EncodedAppend {
                operation: append.operation,
                record: append.record.clone(),
                tx_id,
                line,
                bytes_written,
            });
        }

        let schema_hash = schema.schema_hash();
        let preamble = encode_six_preamble(table, &schema_hash);
        let append_len = encoded
            .iter()
            .map(|append| append.bytes_written)
            .sum::<u64>();
        let target = self.append_target(table.name(), append_len, preamble.len() as u64)?;
        if let Some(parent) = target.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut append_bytes = Vec::new();
        if target.is_new {
            append_bytes.extend_from_slice(preamble.as_bytes());
        }
        let mut offset = target.row_offset;
        let mut results = Vec::with_capacity(encoded.len());

        for append in &encoded {
            let id = record_id(&append.record)?;
            let old_record = if cache.has_row(&id) {
                self.cached_record(table.name(), &id)?
            } else {
                None
            };
            let ptr = RowPointer {
                chunk_name: target.chunk_name.clone(),
                offset,
                len: append.bytes_written as u32,
                tx_id: append.tx_id,
            };
            append_bytes.extend_from_slice(append.line.as_bytes());
            append_bytes.push(b'\n');
            cache.apply_operation(
                table,
                append.operation,
                &append.record,
                ptr,
                old_record.as_ref(),
            )?;
            results.push(AppendResult {
                tx_id: append.tx_id,
                operation: append.operation,
                bytes_written: append.bytes_written,
            });
            offset = offset.saturating_add(append.bytes_written);
        }

        let mut hash_bytes = Vec::new();
        for append in &encoded {
            hash_bytes.extend_from_slice(target.chunk_name.as_bytes());
            hash_bytes.push(0);
            hash_bytes.extend_from_slice(append.line.as_bytes());
            hash_bytes.push(b'\n');
        }
        cache.source_hash = extend_source_hash(&cache.source_hash, &hash_bytes)?;
        cache.version = SIXB_BINARY_VERSION;
        cache.table = table.name().to_owned();
        cache.schema_hash = schema_hash;

        if target.is_new {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target.path)?;
            file.write_all(&append_bytes)?;
            file.sync_data()?;
            self.remember_chunk(table.name(), &target.chunk_name, append_bytes)?;
        } else {
            let mut file = OpenOptions::new().append(true).open(&target.path)?;
            file.write_all(&append_bytes)?;
            file.sync_data()?;
            self.set_chunk_len(
                table.name(),
                &target.chunk_name,
                target.row_offset + append_len,
            )?;
            self.forget_chunk(table.name(), &target.chunk_name)?;
        }
        self.forget_sixb_cache(table.name())?;
        self.remember_runtime_sixb_cache(cache)?;
        for append in &encoded {
            let id = record_id(&append.record)?;
            match append.operation {
                Operation::Put => self.remember_record(&append.record)?,
                Operation::Delete => self.forget_record(table.name(), &id)?,
            }
        }
        let next_tx = tx_start + encoded.len() as u64;
        self.set_next_tx_id(next_tx)?;
        self.set_next_chunk_counter(table.name(), target.next_chunk)?;
        if encoded.len() > 1 || !self.metadata_path().exists() {
            self.write_metadata(schema, next_tx)?;
        }

        Ok(results)
    }

    pub(super) fn append_target(
        &self,
        table_name: &str,
        append_len: u64,
        preamble_len: u64,
    ) -> io::Result<AppendTarget> {
        let next_chunk = self.next_chunk_counter(table_name)?;
        if next_chunk > 0 {
            let chunk_counter = next_chunk - 1;
            let chunk_relative_path = chunk_path(chunk_counter)?;
            let chunk_name = chunk_path_to_name(&chunk_relative_path)?;
            let path = self.table_dir(table_name).join(&chunk_relative_path);
            if path.exists() {
                let current_len =
                    if let Some(len) = self.cached_chunk_len(table_name, &chunk_name)? {
                        len
                    } else {
                        let len = fs::metadata(&path)?.len();
                        self.set_chunk_len(table_name, &chunk_name, len)?;
                        len
                    };
                if current_len.saturating_add(append_len) <= MAX_SIX_CHUNK_BYTES {
                    return Ok(AppendTarget {
                        chunk_name,
                        path,
                        row_offset: current_len,
                        next_chunk,
                        is_new: false,
                    });
                }
            }
        }

        let chunk_counter = next_chunk;
        let chunk_relative_path = chunk_path(chunk_counter)?;
        let chunk_name = chunk_path_to_name(&chunk_relative_path)?;
        Ok(AppendTarget {
            chunk_name,
            path: self.table_dir(table_name).join(&chunk_relative_path),
            row_offset: preamble_len,
            next_chunk: chunk_counter.saturating_add(1),
            is_new: true,
        })
    }

    pub(super) fn truncate_incomplete_tail(&self, table_name: &str) -> io::Result<()> {
        let Some(path) = six_files_in_read_order(&self.table_dir(table_name))?
            .into_iter()
            .next_back()
        else {
            return Ok(());
        };
        let bytes = fs::read(&path)?;
        if bytes.is_empty() || bytes.ends_with(b"\n") {
            return Ok(());
        }
        let complete_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let file = OpenOptions::new().write(true).open(&path)?;
        file.set_len(complete_len as u64)?;
        file.sync_data()?;
        let chunk_name = relative_chunk_name(&self.table_dir(table_name), &path)?;
        self.forget_chunk(table_name, &chunk_name)?;
        self.set_chunk_len(table_name, &chunk_name, complete_len as u64)
    }

    pub(super) fn ensure_complete_tail(&self, table_name: &str) -> io::Result<()> {
        let Some(path) = six_files_in_read_order(&self.table_dir(table_name))?
            .into_iter()
            .next_back()
        else {
            return Ok(());
        };
        let mut file = File::open(&path)?;
        if file.metadata()?.len() == 0 {
            return Ok(());
        }
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last)?;
        if last[0] == b'\n' {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("incomplete .6 tail in clean workspace: {}", path.display()),
        ))
    }

    pub(super) fn prepare_workspace_for_write(
        &self,
        schema: &DatabaseSchema,
        target_table: &str,
        recovering_dirty_workspace: bool,
    ) -> io::Result<()> {
        let schema_hash = schema.schema_hash();
        if recovering_dirty_workspace {
            for table in schema.tables().values() {
                self.ensure_table_layout(table, &schema_hash)?;
                self.truncate_incomplete_tail(table.name())?;
            }
            self.invalidate_cached_state()?;
        } else {
            let table = schema
                .table(target_table)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown table"))?;
            self.ensure_table_layout(table, &schema_hash)?;
            self.ensure_complete_tail(table.name())?;
        }
        Ok(())
    }
}
