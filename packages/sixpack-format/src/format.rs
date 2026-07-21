//! sixpack file format implementation.
//!
//! The current durable row segment format is `.6`: tab-separated, one row per
//! line, with explicit escaping for tabs/newlines inside values. Legacy JSONL
//! helpers remain here while older tests and prototypes still use them.

use std::fmt;

use sixpack_core::{PrimitiveType, Record, TableSchema, Value};

/// File format version recognized by this shell.
pub const FORMAT_VERSION: u32 = 1;
pub const SIX_MAGIC: &str = "SIX";
pub const SIX_PROFILE_TABLE: &str = "table";
pub const SIXB_MAGIC: &str = "SIXB";
pub const SIXB_BINARY_VERSION: u32 = 2;
mod error;
#[cfg(feature = "legacy-jsonl")]
mod legacy_jsonl;
mod search;
mod sixb;

pub use error::FormatError;
#[cfg(feature = "legacy-jsonl")]
pub use legacy_jsonl::*;
pub use search::SIXX_MAGIC;
pub use sixb::*;

/// Internal operation type in the JSONL append log.
#[cfg_attr(feature = "legacy-jsonl", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "legacy-jsonl", serde(rename_all = "lowercase"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Adds/replaces row data.
    Put,
    /// Marks a row as deleted.
    Delete,
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Put => write!(formatter, "put"),
            Self::Delete => write!(formatter, "delete"),
        }
    }
}

/// Returns the exact `.6` header for a table.
pub fn encode_six_header(table: &TableSchema) -> String {
    table.field_order().join("\t")
}

/// Encodes the self-describing `.6` preamble for one logical table segment.
pub fn encode_six_preamble(table: &TableSchema, schema_hash: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{SIX_MAGIC}\t{FORMAT_VERSION}\t{SIX_PROFILE_TABLE}\t{}\t{schema_hash}\n",
        escape_six_value(table.name())
    ));
    for field_name in table.field_order() {
        let field = table
            .field(field_name)
            .expect("field order only contains declared fields");
        out.push_str(&format!(
            "@field\t{}\t{}\n",
            escape_six_value(field.name()),
            <&'static str>::from(field.kind())
        ));
    }
    for lookup in table.lookup_specs_with_implicit_id() {
        out.push_str(&format!(
            "@lookup\t{}\t{}\n",
            escape_six_value(lookup.field_name()),
            if lookup.unique() { "unique" } else { "many" }
        ));
    }
    out.push_str("@data\n");
    out
}

/// Validates the complete self-describing preamble for a `.6` table segment.
pub fn validate_six_preamble(
    table: &TableSchema,
    schema_hash: &str,
    preamble: &str,
) -> Result<(), FormatError> {
    let expected = encode_six_preamble(table, schema_hash);
    let mut actual_lines = preamble.lines();
    let mut expected_lines = expected.lines();
    let actual_magic = actual_lines.next().unwrap_or_default();
    let expected_magic = expected_lines.next().unwrap_or_default();
    let actual_parts = actual_magic.split('\t').collect::<Vec<_>>();
    let expected_parts = expected_magic.split('\t').collect::<Vec<_>>();
    let valid_magic = actual_parts.len() == 5
        && expected_parts.len() == 5
        && actual_parts[..4] == expected_parts[..4]
        && actual_parts[4].len() == 16
        && actual_parts[4].bytes().all(|byte| byte.is_ascii_hexdigit());
    let valid_table_shape = actual_lines.eq(expected_lines);
    if valid_magic && valid_table_shape {
        Ok(())
    } else {
        Err(FormatError::BadSixMagic(
            preamble.lines().next().unwrap_or_default().to_owned(),
        ))
    }
}

/// Returns true when a line is the magic line for the current `.6` table profile.
pub fn is_six_magic_line(line: &str) -> bool {
    line.starts_with("SIX\t")
}

/// Encodes one `.6` data row in schema field order.
pub fn encode_six_row(table: &TableSchema, record: &Record) -> Result<String, FormatError> {
    let mut columns = Vec::with_capacity(table.field_order().len());
    for field_name in table.field_order() {
        let value = record
            .fields()
            .get(field_name)
            .ok_or_else(|| FormatError::BadSixRecord(format!("missing field `{field_name}`")))?;
        columns.push(escape_six_value(&value_to_string(value)));
    }
    Ok(columns.join("\t"))
}

/// Encodes one `.6` operation line.
pub fn encode_six_operation(
    table: &TableSchema,
    operation: Operation,
    tx_id: u64,
    record: &Record,
) -> Result<String, FormatError> {
    match operation {
        Operation::Put => {
            let row = encode_six_row(table, record)?;
            Ok(format!("R\t{tx_id}\t{row}"))
        }
        Operation::Delete => {
            let id = record
                .fields()
                .get("id")
                .ok_or_else(|| FormatError::BadSixRecord("delete missing id".to_owned()))?;
            Ok(format!(
                "D\t{tx_id}\t{}",
                escape_six_value(&value_to_string(id))
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SixOperationRecord {
    Put { tx_id: u64, record: Record },
    Delete { tx_id: u64, id: String },
}

impl SixOperationRecord {
    pub fn tx_id(&self) -> u64 {
        match self {
            Self::Put { tx_id, .. } | Self::Delete { tx_id, .. } => *tx_id,
        }
    }
}

/// Parses one current-profile `.6` operation line.
pub fn decode_six_operation(
    table: &TableSchema,
    line: &str,
) -> Result<SixOperationRecord, FormatError> {
    let mut fixed = line.splitn(3, '\t');
    let tag = fixed.next().unwrap_or_default();
    let tx = fixed.next().ok_or(FormatError::BadSixColumnCount {
        expected: 3,
        found: 1,
    })?;
    let tail = fixed.next().ok_or(FormatError::BadSixColumnCount {
        expected: 3,
        found: 2,
    })?;
    let tx_id = tx.parse::<u64>().map_err(FormatError::BadSixTx)?;
    match tag {
        "R" => Ok(SixOperationRecord::Put {
            tx_id,
            record: decode_six_row(table, tail)?,
        }),
        "D" => Ok(SixOperationRecord::Delete {
            tx_id,
            id: unescape_six_value(tail)?,
        }),
        other => Err(FormatError::BadSixOperation(other.to_owned())),
    }
}

/// Parses one `.6` row into a typed record.
pub fn decode_six_row(table: &TableSchema, line: &str) -> Result<Record, FormatError> {
    let parts: Vec<_> = line.split('\t').collect();
    let expected = table.field_order().len();
    if parts.len() != expected {
        return Err(FormatError::BadSixColumnCount {
            expected,
            found: parts.len(),
        });
    }

    let mut record = Record::new(table.name());
    for (index, field_name) in table.field_order().iter().enumerate() {
        let field = table
            .field(field_name)
            .expect("field order only contains declared fields");
        let raw = unescape_six_value(parts[index])?;
        let value = parse_ten_value(field.kind(), field_name, &raw)?;
        record
            .insert_field(field_name, value)
            .map_err(|error| FormatError::BadSixRecord(error.to_string()))?;
    }

    Ok(record)
}

/// Escapes one `.6` field value.
pub fn escape_six_value(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

/// Unescapes one `.6` field value.
pub fn unescape_six_value(value: &str) -> Result<String, FormatError> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(FormatError::BadSixEscape("dangling \\".to_owned()));
        };
        match escaped {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            other => return Err(FormatError::BadSixEscape(format!("\\{other}"))),
        }
    }
    Ok(out)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Id(value) => value.clone(),
        Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

fn parse_ten_value(kind: PrimitiveType, field: &str, value: &str) -> Result<Value, FormatError> {
    match kind {
        PrimitiveType::Id => Ok(Value::Id(value.to_owned())),
        PrimitiveType::Text => Ok(Value::Text(value.to_owned())),
        PrimitiveType::Int => {
            value
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Float => {
            value
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
        PrimitiveType::Bool => {
            value
                .parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| FormatError::BadSixValue {
                    field: field.to_owned(),
                    kind,
                    value: value.to_owned(),
                })
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/unit.rs"]
mod tests;
