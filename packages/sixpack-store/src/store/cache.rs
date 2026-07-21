use super::*;

impl LocalStore {
    pub(super) fn write_sixb_cache(
        &self,
        table_name: &str,
        cache: &SixbCache,
    ) -> io::Result<Arc<SixbCache>> {
        let path = self.sixb_path(table_name);
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("sixb.tmp-{}-{counter}", std::process::id()));
        fs::write(&tmp, encode_sixb_cache(cache))?;
        fs::rename(tmp, path)?;
        self.remember_sixb_cache(cache.clone())
    }

    pub(super) fn read_chunk(
        &self,
        table_name: &str,
        chunk_name: &str,
    ) -> io::Result<Arc<Vec<u8>>> {
        let key = (table_name.to_owned(), chunk_name.to_owned());
        if let Some(chunk) = self
            .chunk_cache
            .read()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .get(&key)
            .cloned()
        {
            return Ok(chunk);
        }

        let path = self.table_dir(table_name).join(chunk_name);
        let bytes = Arc::new(fs::read(path)?);
        self.set_chunk_len(table_name, chunk_name, bytes.len() as u64)?;
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .insert(key, Arc::clone(&bytes));
        Ok(bytes)
    }

    pub(super) fn remember_chunk(
        &self,
        table_name: &str,
        chunk_name: &str,
        bytes: Vec<u8>,
    ) -> io::Result<()> {
        let len = bytes.len() as u64;
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .insert(
                (table_name.to_owned(), chunk_name.to_owned()),
                Arc::new(bytes),
            );
        self.set_chunk_len(table_name, chunk_name, len)?;
        Ok(())
    }

    pub(super) fn forget_chunk(&self, table_name: &str, chunk_name: &str) -> io::Result<()> {
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .remove(&(table_name.to_owned(), chunk_name.to_owned()));
        self.chunk_len_cache
            .write()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .remove(&(table_name.to_owned(), chunk_name.to_owned()));
        Ok(())
    }

    #[cfg(feature = "experimental-compaction")]
    pub(super) fn forget_table_chunks(&self, table_name: &str) -> io::Result<()> {
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .retain(|(table, _), _| table != table_name);
        self.chunk_len_cache
            .write()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .retain(|(table, _), _| table != table_name);
        Ok(())
    }

    pub(super) fn cached_chunk_len(
        &self,
        table_name: &str,
        chunk_name: &str,
    ) -> io::Result<Option<u64>> {
        Ok(self
            .chunk_len_cache
            .read()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .get(&(table_name.to_owned(), chunk_name.to_owned()))
            .copied())
    }

    pub(super) fn set_chunk_len(
        &self,
        table_name: &str,
        chunk_name: &str,
        len: u64,
    ) -> io::Result<()> {
        self.chunk_len_cache
            .write()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .insert((table_name.to_owned(), chunk_name.to_owned()), len);
        Ok(())
    }

    pub(super) fn cached_sixb(
        &self,
        table_name: &str,
        schema_hash: &str,
    ) -> io::Result<Option<Arc<SixbCache>>> {
        let guard = self
            .sixb_cache
            .read()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?;
        Ok(guard.get(table_name).and_then(|cache| {
            (cache.version == SIXB_BINARY_VERSION
                && cache.table == table_name
                && cache.schema_hash == schema_hash)
                .then(|| Arc::clone(cache))
        }))
    }

    pub(super) fn cached_source_hash(&self, table_name: &str) -> io::Result<Option<String>> {
        Ok(self
            .sixb_cache
            .read()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.source_hash.clone()))
    }

    pub(super) fn discovered_next_tx_id(&self) -> io::Result<u64> {
        let tables_dir = self.database_dir().join("tables");
        if !tables_dir.exists() {
            return Ok(1);
        }

        let mut max_tx = 0u64;
        for table_entry in fs::read_dir(tables_dir)? {
            let table_entry = table_entry?;
            let table_dir = table_entry.path();
            if !table_dir.is_dir() {
                continue;
            }
            for path in six_files_in_read_order(&table_dir)? {
                max_tx = max_tx.max(max_tx_in_six_file(&path)?);
            }
        }
        Ok(max_tx.saturating_add(1).max(1))
    }

    pub(super) fn remember_sixb_cache(&self, cache: SixbCache) -> io::Result<Arc<SixbCache>> {
        let cache = Arc::new(cache);
        let mut guard = self
            .sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?;
        guard.insert(cache.table.clone(), Arc::clone(&cache));
        Ok(cache)
    }

    pub(super) fn set_next_tx_id(&self, next_tx: u64) -> io::Result<()> {
        *self
            .next_tx_cache
            .write()
            .map_err(|_| io::Error::other("next tx cache lock poisoned"))? = Some(next_tx);
        Ok(())
    }

    pub(super) fn set_next_chunk_counter(
        &self,
        table_name: &str,
        next_chunk: u64,
    ) -> io::Result<()> {
        self.next_chunk_cache
            .write()
            .map_err(|_| io::Error::other("next chunk cache lock poisoned"))?
            .insert(table_name.to_owned(), next_chunk);
        Ok(())
    }

    pub(super) fn cached_record(&self, table_name: &str, id: &str) -> io::Result<Option<Record>> {
        Ok(self
            .row_cache
            .read()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .get(table_name)
            .and_then(|table| table.get(id))
            .cloned())
    }

    pub(super) fn runtime_sixb_to_cache(
        &self,
        table_name: &str,
        schema_hash: &str,
    ) -> io::Result<Option<SixbCache>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .filter(|cache| cache.schema_hash == schema_hash)
            .map(RuntimeSixb::to_cache))
    }

    pub(super) fn runtime_row_entries(
        &self,
        table_name: &str,
    ) -> io::Result<Option<Vec<SixbRowEntry>>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(RuntimeSixb::row_entries))
    }

    pub(super) fn runtime_row_entry(
        &self,
        table_name: &str,
        id: &str,
    ) -> io::Result<Option<SixbRowEntry>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .and_then(|cache| cache.row_entry(id)))
    }

    pub(super) fn runtime_lookup_entries(
        &self,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Vec<SixbLookupEntry>>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.lookup_entries(field_name, key)))
    }

    pub(super) fn runtime_row_count(&self, table_name: &str) -> io::Result<Option<usize>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(RuntimeSixb::row_count))
    }

    pub(super) fn runtime_lookup_count(
        &self,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<usize>> {
        Ok(self
            .runtime_sixb_cache
            .read()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .get(table_name)
            .map(|cache| cache.lookup_count(field_name, key)))
    }

    pub(super) fn forget_sixb_cache(&self, table_name: &str) -> io::Result<()> {
        self.sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .remove(table_name);
        Ok(())
    }

    #[cfg(feature = "experimental-compaction")]
    pub(super) fn forget_runtime_sixb_cache(&self, table_name: &str) -> io::Result<()> {
        self.runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .remove(table_name);
        Ok(())
    }

    pub(super) fn take_sixb_cache_for_write(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<SixbCache> {
        self.ensure_sixb_snapshot(schema, table_name)?;
        let cache = self
            .sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .remove(table_name)
            .ok_or_else(|| io::Error::other("sixb cache missing after ensure"))?;
        Ok(match Arc::try_unwrap(cache) {
            Ok(cache) => cache,
            Err(cache) => cache.as_ref().clone(),
        })
    }

    pub(super) fn take_runtime_sixb_for_write(
        &self,
        schema: &DatabaseSchema,
        table_name: &str,
    ) -> io::Result<RuntimeSixb> {
        if let Some(cache) = self
            .runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .remove(table_name)
        {
            return Ok(cache);
        }

        self.take_sixb_cache_for_write(schema, table_name)
            .map(RuntimeSixb::from_cache)
    }

    pub(super) fn remember_runtime_sixb_cache(&self, cache: RuntimeSixb) -> io::Result<()> {
        self.runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .insert(cache.table.clone(), cache);
        Ok(())
    }

    pub(super) fn remember_record(&self, record: &Record) -> io::Result<()> {
        let id = record_id(record)?;
        self.row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .entry(record.table().to_owned())
            .or_default()
            .insert(id, record.clone());
        Ok(())
    }

    pub(super) fn forget_record(&self, table_name: &str, id: &str) -> io::Result<()> {
        if let Some(table) = self
            .row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .get_mut(table_name)
        {
            table.remove(id);
        }
        Ok(())
    }

    pub(super) fn remember_table_records<'a>(
        &self,
        table_name: &str,
        records: impl IntoIterator<Item = (&'a String, &'a LiveRow)>,
    ) -> io::Result<()> {
        let mut guard = self
            .row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?;
        let table = guard.entry(table_name.to_owned()).or_default();
        table.clear();
        for (id, live) in records {
            table.insert(id.clone(), live.record.clone());
        }
        Ok(())
    }
}
