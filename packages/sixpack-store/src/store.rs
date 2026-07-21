//! Local storage engine implementation.
//!
//! The current store writes readable `.6` table row segments, keeps a small
//! `sixpack.toml` physical layout map, and rebuilds generated `.6b` lookup
//! caches from canonical `.6` data.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use sixpack_core::{DatabaseSchema, Record, TableSchema, Value, WorkspaceName};
use sixpack_format::{
    Operation, RowPointer, SIXB_BINARY_VERSION, SixOperationRecord, SixbCache, SixbLookupEntry,
    SixbRowEntry, decode_six_operation, decode_six_row, decode_sixb_cache, encode_six_header,
    encode_six_operation, encode_six_preamble, encode_sixb_cache, is_six_magic_line, source_hash,
    validate_six_preamble,
};

mod append;
mod cache;
mod coordination;
mod files;
mod index;
mod internal;
mod projection;
mod read;
mod types;
mod write;
use files::*;
use index::RuntimeSixb;
use internal::{AppendTarget, EncodedAppend, LiveRow, ScannedSixEntry, TableScan};
pub use types::*;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const CHUNK_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const CHUNK_BASE: usize = 36;
const CHUNK_WIDTH: usize = 3;
const MAX_CHUNKS: u64 = 36u64.pow(CHUNK_WIDTH as u32);
const MAX_SIX_CHUNK_BYTES: u64 = 1024 * 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

type TableName = String;
type RowId = String;
type ChunkName = String;
type CachedChunk = Arc<Vec<u8>>;

/// Local store handle.
#[derive(Clone)]
pub struct LocalStore {
    root: PathBuf,
    workspace: String,
    sixb_cache: Arc<RwLock<BTreeMap<TableName, Arc<SixbCache>>>>,
    runtime_sixb_cache: Arc<RwLock<BTreeMap<TableName, RuntimeSixb>>>,
    row_cache: Arc<RwLock<BTreeMap<TableName, BTreeMap<RowId, Record>>>>,
    chunk_cache: Arc<RwLock<BTreeMap<(TableName, ChunkName), CachedChunk>>>,
    chunk_len_cache: Arc<RwLock<BTreeMap<(TableName, ChunkName), u64>>>,
    next_tx_cache: Arc<RwLock<Option<u64>>>,
    next_chunk_cache: Arc<RwLock<BTreeMap<String, u64>>>,
    layout_cache: Arc<RwLock<BTreeSet<(String, String)>>>,
    workspace_gate: Arc<RwLock<()>>,
    observed_revision: Arc<RwLock<Option<u64>>>,
    active_schema_hash: Arc<RwLock<Option<String>>>,
}

impl std::fmt::Debug for LocalStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStore")
            .field("root", &self.root)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LocalStore {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.workspace == other.workspace
    }
}

impl Eq for LocalStore {}

struct WorkspaceReadGuard<'a> {
    _process: RwLockReadGuard<'a, ()>,
    _file: File,
}

struct WorkspaceWriteGuard<'a> {
    _process: RwLockWriteGuard<'a, ()>,
    _file: File,
}

impl LocalStore {
    /// Creates a store handle without touching the filesystem.
    pub fn new(root: impl Into<PathBuf>, workspace: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            workspace: workspace.into(),
            sixb_cache: Arc::new(RwLock::new(BTreeMap::new())),
            runtime_sixb_cache: Arc::new(RwLock::new(BTreeMap::new())),
            row_cache: Arc::new(RwLock::new(BTreeMap::new())),
            chunk_cache: Arc::new(RwLock::new(BTreeMap::new())),
            chunk_len_cache: Arc::new(RwLock::new(BTreeMap::new())),
            next_tx_cache: Arc::new(RwLock::new(None)),
            next_chunk_cache: Arc::new(RwLock::new(BTreeMap::new())),
            layout_cache: Arc::new(RwLock::new(BTreeSet::new())),
            workspace_gate: Arc::new(RwLock::new(())),
            observed_revision: Arc::new(RwLock::new(None)),
            active_schema_hash: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns the store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the workspace name associated with this store.
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// Database directory for this workspace.
    pub fn database_dir(&self) -> PathBuf {
        self.root.join(&self.workspace)
    }

    /// Root `sixpack.toml` metadata path.
    pub fn metadata_path(&self) -> PathBuf {
        self.database_dir().join("sixpack.toml")
    }

    /// Table directory.
    pub fn table_dir(&self, table: &str) -> PathBuf {
        self.database_dir().join("tables").join(table)
    }

    /// Deterministic `.6` chunk path for a table counter.
    pub fn chunk_six_path(&self, table: &str, chunk_counter: u64) -> io::Result<PathBuf> {
        Ok(self.table_dir(table).join(chunk_path(chunk_counter)?))
    }

    /// Generated binary cache for table indexes/lookups.
    pub fn sixb_path(&self, table: &str) -> PathBuf {
        self.generated_index_path(table, GeneratedIndexKind::Lookup)
    }

    /// Reserved generated full-text search index path.
    ///
    /// The `.6x` boundary is stable, but full-text search is not implemented.
    pub fn sixx_path(&self, table: &str) -> PathBuf {
        self.generated_index_path(table, GeneratedIndexKind::FullText)
    }

    /// Path for one generated index family.
    pub fn generated_index_path(&self, table: &str, kind: GeneratedIndexKind) -> PathBuf {
        self.database_dir()
            .join("engine")
            .join(format!("{table}.{}", kind.extension()))
    }

    /// Cross-process coordination file for this workspace.
    pub fn workspace_lock_path(&self) -> PathBuf {
        self.database_dir().join("engine").join("workspace.lock")
    }

    /// Commit marker used to invalidate caches in other processes.
    pub fn revision_path(&self) -> PathBuf {
        self.database_dir().join("engine").join("revision")
    }

    /// Creates DB directory layout if needed.
    pub fn ensure_workspace_layout(&self) -> io::Result<()> {
        WorkspaceName::new(&self.workspace)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        fs::create_dir_all(self.database_dir().join("tables"))?;
        fs::create_dir_all(self.database_dir().join("engine"))
    }

    /// Initializes an empty database layout for every table in the schema.
    pub fn init(&self, schema: &DatabaseSchema) -> io::Result<()> {
        let _guard = self.workspace_write_guard()?;
        self.refresh_if_revision_changed()?;
        self.activate_schema(schema)?;
        self.ensure_workspace_layout()?;
        let schema_hash = schema.schema_hash();
        for table in schema.tables().values() {
            self.ensure_table_layout(table, &schema_hash)?;
            self.rebuild_sixb_inner(schema, table.name())?;
        }
        let next_tx = self.next_tx_id()?;
        self.write_metadata(schema, next_tx)?;
        self.publish_revision(next_tx)
    }

    /// Computes next transaction id from private engine metadata.
    pub fn next_tx_id(&self) -> io::Result<u64> {
        if let Some(value) = *self
            .next_tx_cache
            .read()
            .map_err(|_| io::Error::other("next tx cache lock poisoned"))?
        {
            return Ok(value);
        }
        let metadata = self.metadata_path();
        if !metadata.exists() {
            let recovered = self.discovered_next_tx_id()?;
            self.set_next_tx_id(recovered)?;
            return Ok(recovered);
        }
        let file = File::open(metadata)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some(value) = line.strip_prefix("next_tx = ") else {
                continue;
            };
            let parsed = value.trim().parse::<u64>().map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("bad next_tx: {error}"))
            })?;
            let recovered = parsed.max(self.discovered_next_tx_id()?);
            self.set_next_tx_id(recovered)?;
            return Ok(recovered);
        }
        let recovered = self.discovered_next_tx_id()?;
        self.set_next_tx_id(recovered)?;
        Ok(recovered)
    }

    /// Computes the next chunk counter for one table from metadata, falling back to files.
    pub fn next_chunk_counter(&self, table_name: &str) -> io::Result<u64> {
        if let Some(value) = self
            .next_chunk_cache
            .read()
            .map_err(|_| io::Error::other("next chunk cache lock poisoned"))?
            .get(table_name)
            .copied()
        {
            return Ok(value);
        }
        let discovered_next = six_files_in_read_order(&self.table_dir(table_name))?.len() as u64;
        let metadata = self.metadata_path();
        if metadata.exists() {
            let file = File::open(metadata)?;
            let mut in_table = false;
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.starts_with("[tables.") {
                    in_table = line == format!("[tables.{table_name}]");
                    continue;
                }
                if in_table {
                    let Some(value) = line.strip_prefix("next_chunk = ") else {
                        continue;
                    };
                    let parsed = value
                        .trim()
                        .parse::<u64>()
                        .map_err(|error| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("bad next_chunk for `{table_name}`: {error}"),
                            )
                        })?
                        .max(discovered_next);
                    self.set_next_chunk_counter(table_name, parsed)?;
                    return Ok(parsed);
                }
            }
        }
        self.set_next_chunk_counter(table_name, discovered_next)?;
        Ok(discovered_next)
    }

    fn write_metadata(&self, schema: &DatabaseSchema, next_tx: u64) -> io::Result<()> {
        let tmp = self.metadata_path().with_extension("toml.tmp");
        let mut out = String::new();
        out.push_str("version = 1\n");
        out.push_str(&format!("schema_hash = \"{}\"\n", schema.schema_hash()));
        out.push_str(&format!("next_tx = {next_tx}\n\n"));

        for (index, table) in schema.tables().values().enumerate() {
            let table_id = index + 1;
            out.push_str(&format!("[tables.{}]\n", table.name()));
            out.push_str(&format!("id = {table_id}\n"));
            out.push_str(&format!("path = \"tables/{}\"\n", table.name()));
            out.push_str(&format!(
                "next_chunk = {}\n",
                self.next_chunk_counter(table.name())?
            ));
            out.push_str(&format!(
                "header = \"{}\"\n\n",
                escape_toml(&encode_six_header(table))
            ));
            out.push_str(&format!("[tables.{}.index]\n", table.name()));
            out.push_str("state = \"ready\"\n");
            out.push_str(&format!("file = \"engine/{}.6b\"\n", table.name()));
            if let Some(source_hash) = self.cached_source_hash(table.name())? {
                out.push_str(&format!(
                    "source_hash = \"{}\"\n",
                    escape_toml(&source_hash)
                ));
            }
            out.push('\n');
        }

        fs::write(&tmp, out)?;
        fs::rename(tmp, self.metadata_path())
    }

    fn scan_table_files(&self, table: &TableSchema, schema_hash: &str) -> io::Result<TableScan> {
        let mut live = BTreeMap::new();
        let mut hash_bytes = Vec::new();
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            verify_header(table, schema_hash, &path)?;
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            let entries = scan_six_file(table, &path, &chunk_name)?;
            for entry in entries {
                hash_bytes.extend_from_slice(chunk_name.as_bytes());
                hash_bytes.push(0);
                hash_bytes.extend_from_slice(&entry.raw_line);
                match entry.operation {
                    SixOperationRecord::Put { tx_id: _, record } => {
                        let id = record_id(&record)?;
                        live.insert(
                            id,
                            LiveRow {
                                record,
                                ptr: entry.ptr,
                            },
                        );
                    }
                    SixOperationRecord::Delete { tx_id: _, id } => {
                        live.remove(&id);
                    }
                }
            }
        }
        Ok(TableScan {
            source_hash: source_hash(&hash_bytes),
            live,
        })
    }

    fn scan_table_source_hash(&self, table: &TableSchema, schema_hash: &str) -> io::Result<String> {
        let mut hash_bytes = Vec::new();
        for path in six_files_in_read_order(&self.table_dir(table.name()))? {
            verify_header(table, schema_hash, &path)?;
            let chunk_name = relative_chunk_name(&self.table_dir(table.name()), &path)?;
            for line in raw_six_data_lines(table, &path)? {
                hash_bytes.extend_from_slice(chunk_name.as_bytes());
                hash_bytes.push(0);
                hash_bytes.extend_from_slice(&line);
            }
        }
        Ok(source_hash(&hash_bytes))
    }

    fn read_row_pointer(&self, table: &TableSchema, ptr: &RowPointer) -> io::Result<Record> {
        let chunk = self.read_chunk(table.name(), &ptr.chunk_name)?;
        let start = ptr.offset as usize;
        let end = start.checked_add(ptr.len as usize).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "row pointer offset overflow")
        })?;
        if end > chunk.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row pointer extends past chunk",
            ));
        }
        let mut bytes = chunk[start..end].to_vec();
        if matches!(bytes.last(), Some(b'\n')) {
            bytes.pop();
        }
        let line = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        match decode_six_operation(table, &line).map_err(format_error_to_io)? {
            SixOperationRecord::Put { record, .. } => Ok(record),
            SixOperationRecord::Delete { .. } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row pointer referenced a delete tombstone",
            )),
        }
    }

    fn read_row_entry(&self, table: &TableSchema, entry: &SixbRowEntry) -> io::Result<Record> {
        if let Some(record) = self.cached_record(table.name(), &entry.id)? {
            return Ok(record);
        }
        let record = self.read_row_pointer(table, &entry.ptr)?;
        self.remember_record(&record)?;
        Ok(record)
    }
}

#[cfg(test)]
#[path = "../tests/support/unit.rs"]
mod tests;
