use std::collections::{BTreeMap, BTreeSet};
use std::io;

use sixpack_core::{Record, TableSchema};
use sixpack_format::{Operation, RowPointer, SixbCache, SixbLookupEntry, SixbRowEntry};

use super::{RowId, record_id, value_to_lookup_key};

#[derive(Debug, Clone)]
pub(super) struct RuntimeSixb {
    pub(super) version: u32,
    pub(super) table: String,
    pub(super) schema_hash: String,
    pub(super) source_hash: String,
    pub(super) rows_by_id: BTreeMap<RowId, RowPointer>,
    pub(super) lookup_ids: BTreeMap<(String, String), BTreeSet<RowId>>,
    pub(super) row_lookup_keys: BTreeMap<RowId, Vec<(String, String)>>,
}

impl RuntimeSixb {
    pub(super) fn from_cache(cache: SixbCache) -> Self {
        let mut lookup_ids = BTreeMap::<(String, String), BTreeSet<RowId>>::new();
        let mut row_lookup_keys = BTreeMap::<RowId, Vec<(String, String)>>::new();
        for lookup in cache.lookups {
            let key = (lookup.field_name, lookup.key);
            lookup_ids
                .entry(key.clone())
                .or_default()
                .insert(lookup.id.clone());
            row_lookup_keys.entry(lookup.id).or_default().push(key);
        }

        Self {
            version: cache.version,
            table: cache.table,
            schema_hash: cache.schema_hash,
            source_hash: cache.source_hash,
            rows_by_id: cache
                .rows
                .into_iter()
                .map(|entry| (entry.id, entry.ptr))
                .collect(),
            lookup_ids,
            row_lookup_keys,
        }
    }

    pub(super) fn to_cache(&self) -> SixbCache {
        let rows = self
            .rows_by_id
            .iter()
            .map(|(id, ptr)| SixbRowEntry {
                id: id.clone(),
                ptr: ptr.clone(),
            })
            .collect();
        let mut lookups = Vec::new();
        for ((field_name, key), ids) in &self.lookup_ids {
            for id in ids {
                lookups.push(SixbLookupEntry {
                    field_name: field_name.clone(),
                    key: key.clone(),
                    id: id.clone(),
                });
            }
        }
        SixbCache {
            version: self.version,
            table: self.table.clone(),
            schema_hash: self.schema_hash.clone(),
            source_hash: self.source_hash.clone(),
            rows,
            lookups,
        }
    }

    pub(super) fn has_row(&self, id: &str) -> bool {
        self.rows_by_id.contains_key(id)
    }

    pub(super) fn first_lookup_id(&self, field_name: &str, key: &str) -> Option<&str> {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .and_then(|ids| ids.first())
            .map(String::as_str)
    }

    pub(super) fn row_entry(&self, id: &str) -> Option<SixbRowEntry> {
        self.rows_by_id.get(id).map(|ptr| SixbRowEntry {
            id: id.to_owned(),
            ptr: ptr.clone(),
        })
    }

    pub(super) fn row_entries(&self) -> Vec<SixbRowEntry> {
        self.rows_by_id
            .iter()
            .map(|(id, ptr)| SixbRowEntry {
                id: id.clone(),
                ptr: ptr.clone(),
            })
            .collect()
    }

    pub(super) fn lookup_entries(&self, field_name: &str, key: &str) -> Vec<SixbLookupEntry> {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .into_iter()
            .flat_map(|ids| ids.iter())
            .map(|id| SixbLookupEntry {
                field_name: field_name.to_owned(),
                key: key.to_owned(),
                id: id.clone(),
            })
            .collect()
    }

    pub(super) fn lookup_count(&self, field_name: &str, key: &str) -> usize {
        self.lookup_ids
            .get(&(field_name.to_owned(), key.to_owned()))
            .map_or(0, BTreeSet::len)
    }

    pub(super) fn row_count(&self) -> usize {
        self.rows_by_id.len()
    }

    pub(super) fn apply_operation(
        &mut self,
        table: &TableSchema,
        operation: Operation,
        record: &Record,
        ptr: RowPointer,
        old_record: Option<&Record>,
    ) -> io::Result<()> {
        let id = record_id(record)?;
        match operation {
            Operation::Put => {
                if self.rows_by_id.contains_key(&id) {
                    self.remove_record_lookups(table, &id, old_record)?;
                }
                self.rows_by_id.insert(id.clone(), ptr);
                self.insert_record_lookups(table, &id, record)
            }
            Operation::Delete => {
                self.rows_by_id.remove(&id);
                self.remove_record_lookups(table, &id, old_record)
            }
        }
    }

    pub(super) fn remove_row_for_validation(
        &mut self,
        table: &TableSchema,
        id: &str,
    ) -> io::Result<()> {
        if self.rows_by_id.remove(id).is_some() {
            self.remove_record_lookups(table, id, None)?;
        }
        Ok(())
    }

    pub(super) fn insert_record_lookups(
        &mut self,
        table: &TableSchema,
        id: &str,
        record: &Record,
    ) -> io::Result<()> {
        let mut keys = Vec::new();
        for lookup in table.lookup_specs_with_implicit_id() {
            let value = record.fields().get(lookup.field_name()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing lookup field `{}`", lookup.field_name()),
                )
            })?;
            let key = (lookup.field_name().to_owned(), value_to_lookup_key(value));
            self.lookup_ids
                .entry(key.clone())
                .or_default()
                .insert(id.to_owned());
            keys.push(key);
        }
        self.row_lookup_keys.insert(id.to_owned(), keys);
        Ok(())
    }

    pub(super) fn remove_record_lookups(
        &mut self,
        table: &TableSchema,
        id: &str,
        old_record: Option<&Record>,
    ) -> io::Result<()> {
        if let Some(old_record) = old_record {
            let mut keys = Vec::new();
            for lookup in table.lookup_specs_with_implicit_id() {
                let value = old_record
                    .fields()
                    .get(lookup.field_name())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("missing lookup field `{}`", lookup.field_name()),
                        )
                    })?;
                keys.push((lookup.field_name().to_owned(), value_to_lookup_key(value)));
            }
            self.remove_lookup_keys(id, keys);
            return Ok(());
        }

        if let Some(keys) = self.row_lookup_keys.remove(id) {
            self.remove_lookup_keys(id, keys);
            return Ok(());
        }

        self.remove_lookup_id_slow(id);
        Ok(())
    }

    pub(super) fn remove_lookup_keys(&mut self, id: &str, keys: Vec<(String, String)>) {
        for key in &keys {
            let remove_key = if let Some(ids) = self.lookup_ids.get_mut(key) {
                ids.remove(id);
                ids.is_empty()
            } else {
                false
            };
            if remove_key {
                self.lookup_ids.remove(key);
            }
        }
        self.row_lookup_keys.remove(id);
    }

    pub(super) fn remove_lookup_id_slow(&mut self, id: &str) {
        let empty_keys = self
            .lookup_ids
            .iter_mut()
            .filter_map(|(key, ids)| {
                ids.remove(id);
                ids.is_empty().then(|| key.clone())
            })
            .collect::<Vec<_>>();
        for key in empty_keys {
            self.lookup_ids.remove(&key);
        }
        self.row_lookup_keys.remove(id);
    }
}
