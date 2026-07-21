use super::{FormatError, SIXB_BINARY_VERSION, SIXB_MAGIC, unescape_six_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowPointer {
    pub chunk_name: String,
    pub offset: u64,
    pub len: u32,
    pub tx_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbRowEntry {
    pub id: String,
    pub ptr: RowPointer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbLookupEntry {
    pub field_name: String,
    pub key: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SixbCache {
    pub version: u32,
    pub table: String,
    pub schema_hash: String,
    pub source_hash: String,
    pub rows: Vec<SixbRowEntry>,
    pub lookups: Vec<SixbLookupEntry>,
}

pub fn source_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn encode_sixb_cache(cache: &SixbCache) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"SIXB\0");
    write_u32(&mut out, SIXB_BINARY_VERSION);
    write_string(&mut out, &cache.table);
    write_string(&mut out, &cache.schema_hash);
    write_string(&mut out, &cache.source_hash);
    write_u32(&mut out, cache.rows.len() as u32);
    for row in &cache.rows {
        write_string(&mut out, &row.id);
        write_string(&mut out, &row.ptr.chunk_name);
        write_u64(&mut out, row.ptr.offset);
        write_u32(&mut out, row.ptr.len);
        write_u64(&mut out, row.ptr.tx_id);
    }
    write_u32(&mut out, cache.lookups.len() as u32);
    for lookup in &cache.lookups {
        write_string(&mut out, &lookup.field_name);
        write_string(&mut out, &lookup.key);
        write_string(&mut out, &lookup.id);
    }
    out
}

pub fn decode_sixb_cache(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    if bytes.starts_with(b"SIXB\0") {
        return decode_sixb_cache_binary(bytes);
    }
    decode_sixb_cache_text(bytes)
}

fn decode_sixb_cache_binary(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    let mut reader = BinaryReader::new(bytes);
    reader.expect_magic(b"SIXB\0")?;
    let version = reader.read_u32()?;
    let table = reader.read_string()?;
    let schema_hash = reader.read_string()?;
    let source_hash = reader.read_string()?;
    let row_count = reader.read_u32()? as usize;
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        rows.push(SixbRowEntry {
            id: reader.read_string()?,
            ptr: RowPointer {
                chunk_name: reader.read_string()?,
                offset: reader.read_u64()?,
                len: reader.read_u32()?,
                tx_id: reader.read_u64()?,
            },
        });
    }
    let lookup_count = reader.read_u32()? as usize;
    let mut lookups = Vec::with_capacity(lookup_count);
    for _ in 0..lookup_count {
        lookups.push(SixbLookupEntry {
            field_name: reader.read_string()?,
            key: reader.read_string()?,
            id: reader.read_string()?,
        });
    }
    reader.expect_eof()?;
    Ok(SixbCache {
        version,
        table,
        schema_hash,
        source_hash,
        rows,
        lookups,
    })
}

fn decode_sixb_cache_text(bytes: &[u8]) -> Result<SixbCache, FormatError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| FormatError::BadSixb(format!("invalid utf-8: {error}")))?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| FormatError::BadSixb("missing header".to_owned()))?;
    let header_parts: Vec<_> = header.split('\t').collect();
    if header_parts.len() != 5 || header_parts[0] != SIXB_MAGIC {
        return Err(FormatError::BadSixb("bad SIXB header".to_owned()));
    }
    let version = header_parts[1]
        .parse::<u32>()
        .map_err(|error| FormatError::BadSixb(format!("bad version: {error}")))?;
    let mut cache = SixbCache {
        version,
        table: unescape_six_value(header_parts[2])?,
        schema_hash: unescape_six_value(header_parts[3])?,
        source_hash: unescape_six_value(header_parts[4])?,
        rows: Vec::new(),
        lookups: Vec::new(),
    };

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split('\t').collect();
        match parts.first().copied() {
            Some("row") if parts.len() == 6 => {
                cache.rows.push(SixbRowEntry {
                    id: unescape_six_value(parts[1])?,
                    ptr: RowPointer {
                        chunk_name: unescape_six_value(parts[2])?,
                        offset: parts[3].parse::<u64>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row offset: {error}"))
                        })?,
                        len: parts[4].parse::<u32>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row len: {error}"))
                        })?,
                        tx_id: parts[5].parse::<u64>().map_err(|error| {
                            FormatError::BadSixb(format!("bad row tx: {error}"))
                        })?,
                    },
                });
            }
            Some("lookup") if parts.len() == 4 => {
                cache.lookups.push(SixbLookupEntry {
                    field_name: unescape_six_value(parts[1])?,
                    key: unescape_six_value(parts[2])?,
                    id: unescape_six_value(parts[3])?,
                });
            }
            _ => return Err(FormatError::BadSixb(format!("bad entry: {line}"))),
        }
    }

    Ok(cache)
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect_magic(&mut self, magic: &[u8]) -> Result<(), FormatError> {
        let actual = self.take(magic.len())?;
        if actual == magic {
            Ok(())
        } else {
            Err(FormatError::BadSixb("bad SIXB binary magic".to_owned()))
        }
    }

    fn read_u32(&mut self) -> Result<u32, FormatError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("take returned exact u32 width"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64, FormatError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("take returned exact u64 width"),
        ))
    }

    fn read_string(&mut self) -> Result<String, FormatError> {
        let len = self.read_u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|error| FormatError::BadSixb(format!("invalid utf-8 string: {error}")))
    }

    fn expect_eof(&self) -> Result<(), FormatError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(FormatError::BadSixb("trailing SIXB bytes".to_owned()))
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FormatError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| FormatError::BadSixb("SIXB offset overflow".to_owned()))?;
        if end > self.bytes.len() {
            return Err(FormatError::BadSixb("truncated SIXB binary".to_owned()));
        }
        let out = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(out)
    }
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}
