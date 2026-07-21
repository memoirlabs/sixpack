use std::io;

use sixpack_core::{DatabaseSchema, Record};
use sixpack_format::Operation;

use super::LocalStore;

/// Result of appending one logical row entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendResult {
    /// Assigned transaction id.
    pub tx_id: u64,
    /// Operation used.
    pub operation: Operation,
    /// Bytes written to the `.6` row segment (line + newline).
    pub bytes_written: u64,
}

/// Rows and total count observed from one locked store snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPage {
    pub rows: Vec<Record>,
    pub total: usize,
}

/// Result of compacting one table's append segments.
#[cfg(feature = "experimental-compaction")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    /// Compacted table name.
    pub table: String,
    /// Number of live rows written into the compacted chunk.
    pub live_rows: usize,
    /// Number of `.6` chunks before compaction.
    pub chunks_before: usize,
    /// Number of `.6` chunks after compaction.
    pub chunks_after: usize,
    /// Total `.6` bytes before compaction.
    pub bytes_before: u64,
    /// Total `.6` bytes after compaction.
    pub bytes_after: u64,
    /// Chunk file holding the compacted rows.
    pub chunk_name: String,
}

/// One append-only `.6` operation for a batch write.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendOperation {
    /// Operation to append.
    pub operation: Operation,
    /// Row-like record. Delete operations only require the `id` field.
    pub record: Record,
}

impl AppendOperation {
    /// Creates an append operation.
    pub fn new(operation: Operation, record: Record) -> Self {
        Self { operation, record }
    }

    /// Creates a put operation.
    pub fn put(record: Record) -> Self {
        Self::new(Operation::Put, record)
    }

    /// Creates a delete operation.
    pub fn delete(record: Record) -> Self {
        Self::new(Operation::Delete, record)
    }
}

/// Write-batch conflict mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteBatchMode {
    /// Every put must create a new live id.
    InsertOnly,
    /// Puts replace existing rows and deletes tombstone existing ids.
    Upsert,
}

/// Validated set of operations for one table.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteBatch {
    table: String,
    mode: WriteBatchMode,
    operations: Vec<AppendOperation>,
}

/// Current-state view held under the exclusive workspace write lock.
///
/// Higher layers use this to resolve patch/remove semantics before producing a
/// storage-ready batch. The store itself continues to understand only Put and
/// Delete operations.
pub struct WriteSnapshot<'a> {
    pub(super) store: &'a LocalStore,
    pub(super) schema: &'a DatabaseSchema,
}

impl WriteSnapshot<'_> {
    pub fn get_unique_lookup(
        &self,
        table_name: &str,
        field_name: &str,
        key: &str,
    ) -> io::Result<Option<Record>> {
        self.store
            .get_unique_lookup_inner(self.schema, table_name, field_name, key)
    }
}

impl WriteBatch {
    /// Creates an empty batch for one table.
    pub fn new(table: impl Into<String>, mode: WriteBatchMode) -> Self {
        Self {
            table: table.into(),
            mode,
            operations: Vec::new(),
        }
    }

    /// Creates an insert-only batch of put operations.
    pub fn insert_only(
        table: impl Into<String>,
        records: impl IntoIterator<Item = Record>,
    ) -> io::Result<Self> {
        let mut batch = Self::new(table, WriteBatchMode::InsertOnly);
        for record in records {
            batch.push(AppendOperation::put(record))?;
        }
        Ok(batch)
    }

    /// Creates an upsert batch from append operations.
    pub fn upsert(
        table: impl Into<String>,
        operations: impl IntoIterator<Item = AppendOperation>,
    ) -> io::Result<Self> {
        let mut batch = Self::new(table, WriteBatchMode::Upsert);
        for operation in operations {
            batch.push(operation)?;
        }
        Ok(batch)
    }

    /// Adds one operation, rejecting cross-table batches.
    pub fn push(&mut self, operation: AppendOperation) -> io::Result<()> {
        if operation.record.table() != self.table {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write batch operations must all belong to the same table",
            ));
        }
        self.operations.push(operation);
        Ok(())
    }

    /// Returns the table name.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Returns the conflict mode.
    pub fn mode(&self) -> WriteBatchMode {
        self.mode
    }

    /// Returns the operations.
    pub fn operations(&self) -> &[AppendOperation] {
        &self.operations
    }

    /// Returns whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

/// Generated index families stored under `engine/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedIndexKind {
    /// Exact id and schema-declared lookup projection (`.6b`).
    Lookup,
    /// Optional full-text search projection (`.6x`).
    FullText,
}

impl GeneratedIndexKind {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Lookup => "6b",
            Self::FullText => "6x",
        }
    }
}
