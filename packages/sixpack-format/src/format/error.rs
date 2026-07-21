use std::fmt;

use sixpack_core::PrimitiveType;

/// Error during format serialization/parsing.
#[derive(Debug)]
pub enum FormatError {
    /// Serialization failed.
    #[cfg(feature = "legacy-jsonl")]
    Encode(serde_json::Error),
    /// Deserialization failed.
    #[cfg(feature = "legacy-jsonl")]
    Decode(serde_json::Error),
    /// The log record header/version is not supported.
    UnsupportedVersion { expected: u32, found: u32 },
    /// A `.6` row has the wrong column count.
    BadSixColumnCount { expected: usize, found: usize },
    /// A `.6` file has an invalid magic/header line.
    BadSixMagic(String),
    /// A `.6` row has an invalid transaction id.
    BadSixTx(std::num::ParseIntError),
    /// A `.6` row has an invalid operation.
    BadSixOperation(String),
    /// A `.6` field value cannot be parsed as the schema type.
    BadSixValue {
        field: String,
        kind: PrimitiveType,
        value: String,
    },
    /// A `.6` value has an invalid escape sequence.
    BadSixEscape(String),
    /// A `.6` record could not be built.
    BadSixRecord(String),
    /// A `.6b` cache cannot be decoded.
    BadSixb(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "legacy-jsonl")]
            Self::Encode(error) => write!(formatter, "encode error: {error}"),
            #[cfg(feature = "legacy-jsonl")]
            Self::Decode(error) => write!(formatter, "decode error: {error}"),
            Self::UnsupportedVersion { expected, found } => write!(
                formatter,
                "unsupported format version: expected {expected}, found {found}"
            ),
            Self::BadSixColumnCount { expected, found } => {
                write!(
                    formatter,
                    ".6 row column count mismatch: expected {expected}, found {found}"
                )
            }
            Self::BadSixMagic(line) => write!(formatter, ".6 file has bad magic: {line}"),
            Self::BadSixTx(error) => write!(formatter, ".6 row has invalid tx id: {error}"),
            Self::BadSixOperation(operation) => {
                write!(formatter, ".6 row has invalid operation: {operation}")
            }
            Self::BadSixValue { field, kind, value } => {
                write!(
                    formatter,
                    ".6 field `{field}` expected {kind}, got `{value}`"
                )
            }
            Self::BadSixEscape(value) => write!(formatter, ".6 value has bad escape: {value}"),
            Self::BadSixRecord(error) => write!(formatter, ".6 record error: {error}"),
            Self::BadSixb(error) => write!(formatter, ".6b cache error: {error}"),
        }
    }
}

impl std::error::Error for FormatError {}
