//! Compatibility codec for the pre-`.6` JSONL prototype.
//!
//! This module is feature-gated and is not part of normal database builds.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sixpack_core::{Record, Value};

use super::{FORMAT_VERSION, FormatError, Operation};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    #[serde(rename = "_v")]
    pub version: u32,
    #[serde(rename = "_tx")]
    pub tx_id: u64,
    #[serde(rename = "_op")]
    pub operation: Operation,
    #[serde(rename = "_ts")]
    pub timestamp_ms: u64,
    pub table: String,
    pub data: BTreeMap<String, JsonValue>,
}

impl LogRecord {
    pub fn put(tx_id: u64, record: &Record, timestamp_ms: u64) -> Self {
        Self::new(tx_id, Operation::Put, record, timestamp_ms)
    }

    pub fn delete(tx_id: u64, record: &Record, timestamp_ms: u64) -> Self {
        Self::new(tx_id, Operation::Delete, record, timestamp_ms)
    }

    pub fn new(tx_id: u64, operation: Operation, record: &Record, timestamp_ms: u64) -> Self {
        let data = record
            .fields()
            .iter()
            .map(|(name, value)| (name.clone(), value_to_json(value)))
            .collect();
        Self {
            version: FORMAT_VERSION,
            tx_id,
            operation,
            timestamp_ms,
            table: record.table().to_owned(),
            data,
        }
    }
}

pub fn encode_log_record(record: &LogRecord) -> Result<String, FormatError> {
    serde_json::to_string(record).map_err(FormatError::Encode)
}

pub fn decode_log_record(line: &str) -> Result<LogRecord, FormatError> {
    let parsed: LogRecord = serde_json::from_str(line).map_err(FormatError::Decode)?;
    if parsed.version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion {
            expected: FORMAT_VERSION,
            found: parsed.version,
        });
    }
    Ok(parsed)
}

pub fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(now) => now.as_millis() as u64,
        Err(_) => 0,
    }
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Id(value) | Value::Text(value) => JsonValue::String(value.clone()),
        Value::Int(value) => JsonValue::from(*value),
        Value::Float(value) => JsonValue::from(*value),
        Value::Bool(value) => JsonValue::from(*value),
    }
}
