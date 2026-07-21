use std::collections::BTreeMap;
use std::path::PathBuf;

use sixpack_core::Record;
use sixpack_format::{Operation, RowPointer, SixOperationRecord};

#[derive(Debug, Clone)]
pub(super) struct LiveRow {
    pub(super) record: Record,
    pub(super) ptr: RowPointer,
}

#[derive(Debug, Clone)]
pub(super) struct TableScan {
    pub(super) source_hash: String,
    pub(super) live: BTreeMap<String, LiveRow>,
}

#[derive(Debug, Clone)]
pub(super) struct ScannedSixEntry {
    pub(super) operation: SixOperationRecord,
    pub(super) ptr: RowPointer,
    pub(super) raw_line: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct EncodedAppend {
    pub(super) operation: Operation,
    pub(super) record: Record,
    pub(super) tx_id: u64,
    pub(super) line: String,
    pub(super) bytes_written: u64,
}

#[derive(Debug, Clone)]
pub(super) struct AppendTarget {
    pub(super) chunk_name: String,
    pub(super) path: PathBuf,
    pub(super) row_offset: u64,
    pub(super) next_chunk: u64,
    pub(super) is_new: bool,
}
