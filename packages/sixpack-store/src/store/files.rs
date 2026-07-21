use super::*;

pub(super) fn verify_header(table: &TableSchema, schema_hash: &str, path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header = String::new();
    reader.read_line(&mut header)?;
    let actual = header.trim_end_matches(['\r', '\n']).to_owned();
    let legacy_header = encode_six_header(table);
    if actual == legacy_header {
        return Ok(());
    }

    let mut preamble = header;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete .6 preamble",
            ));
        }
        let is_data = line.trim_end_matches(['\r', '\n']) == "@data";
        preamble.push_str(&line);
        if is_data {
            break;
        }
    }
    validate_six_preamble(table, schema_hash, &preamble).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "expected .6 header/preamble for `{}`, found `{actual}`",
                table.name()
            ),
        )
    })
}

pub(super) fn one_append_result(mut results: Vec<AppendResult>) -> io::Result<AppendResult> {
    results.pop().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "single-operation batch produced no append result",
        )
    })
}

pub(super) fn scan_six_file(
    table: &TableSchema,
    path: &Path,
    chunk_name: &str,
) -> io::Result<Vec<ScannedSixEntry>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut offset = 0u64;

    loop {
        let line_offset = offset;
        let mut line = String::new();
        let len = reader.read_line(&mut line)?;
        if len == 0 {
            break;
        }
        offset += len as u64;
        if !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        if is_six_magic_line(trimmed) || trimmed.starts_with('@') {
            continue;
        }

        let operation = if trimmed.starts_with("R\t") || trimmed.starts_with("D\t") {
            decode_six_operation(table, trimmed).map_err(format_error_to_io)?
        } else if trimmed == encode_six_header(table) {
            continue;
        } else {
            SixOperationRecord::Put {
                tx_id: 0,
                record: decode_six_row(table, trimmed).map_err(format_error_to_io)?,
            }
        };
        let tx_id = operation.tx_id();
        out.push(ScannedSixEntry {
            operation,
            ptr: RowPointer {
                chunk_name: chunk_name.to_owned(),
                offset: line_offset,
                len: len as u32,
                tx_id,
            },
            raw_line: line.into_bytes(),
        });
    }
    Ok(out)
}

pub(super) fn raw_six_data_lines(table: &TableSchema, path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut out = Vec::new();
    let header = encode_six_header(table);

    loop {
        let mut line = String::new();
        let len = reader.read_line(&mut line)?;
        if len == 0 || !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty()
            || is_six_magic_line(trimmed)
            || trimmed.starts_with('@')
            || trimmed == header
        {
            continue;
        }
        out.push(line.into_bytes());
    }
    Ok(out)
}

pub(super) fn max_tx_in_six_file(path: &Path) -> io::Result<u64> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut max_tx = 0u64;

    loop {
        let mut line = String::new();
        let len = reader.read_line(&mut line)?;
        if len == 0 || !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !(trimmed.starts_with("R\t") || trimmed.starts_with("D\t")) {
            continue;
        }
        let mut parts = trimmed.splitn(3, '\t');
        let _tag = parts.next();
        let Some(tx) = parts.next() else {
            continue;
        };
        if let Ok(tx) = tx.parse::<u64>() {
            max_tx = max_tx.max(tx);
        }
    }
    Ok(max_tx)
}

pub(super) fn record_id(record: &Record) -> io::Result<String> {
    let value = record
        .fields()
        .get("id")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "record missing id"))?;
    Ok(value_to_lookup_key(value))
}

pub(super) fn validate_storage_operation(
    schema: &DatabaseSchema,
    table: &TableSchema,
    append: &AppendOperation,
) -> io::Result<()> {
    if append.record.table() != table.name() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "storage operation table does not match its batch",
        ));
    }
    match append.operation {
        Operation::Put => schema
            .validate_record(&append.record)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string())),
        Operation::Delete => match append.record.fields().get("id") {
            Some(Value::Id(_)) => Ok(()),
            Some(value) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("delete id must use `id`, found `{}`", value.value_type()),
            )),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "delete operation is missing id",
            )),
        },
    }
}

pub(super) fn value_to_lookup_key(value: &Value) -> String {
    match value {
        Value::Id(value) | Value::Text(value) => value.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
    }
}

pub(super) fn validate_unique_lookups(
    table: &TableSchema,
    lookups: &[SixbLookupEntry],
) -> io::Result<()> {
    for lookup in table.lookup_specs_with_implicit_id() {
        if !lookup.unique() {
            continue;
        }
        let mut seen = BTreeMap::<(&str, &str), &str>::new();
        for entry in lookups
            .iter()
            .filter(|entry| entry.field_name == lookup.field_name())
        {
            let key = (entry.field_name.as_str(), entry.key.as_str());
            if let Some(existing_id) = seen.insert(key, entry.id.as_str())
                && existing_id != entry.id
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "unique lookup `{}` has duplicate key `{}`",
                        lookup.field_name(),
                        entry.key
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_put_unique_lookup_conflicts(
    table: &TableSchema,
    cache: &RuntimeSixb,
    record: &Record,
    batch_unique: &mut BTreeMap<(String, String), String>,
) -> io::Result<()> {
    let id = record_id(record)?;
    for lookup in table
        .lookup_specs_with_implicit_id()
        .into_iter()
        .filter(|lookup| lookup.unique())
    {
        let value = record.fields().get(lookup.field_name()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing lookup field `{}`", lookup.field_name()),
            )
        })?;
        let key = value_to_lookup_key(value);
        if let Some(conflict_id) = cache.first_lookup_id(lookup.field_name(), &key)
            && conflict_id != id
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unique lookup `{}` key `{}` is already used by row `{}`",
                    lookup.field_name(),
                    key,
                    conflict_id
                ),
            ));
        }
        let unique_key = (lookup.field_name().to_owned(), key);
        if let Some(existing_id) = batch_unique.insert(unique_key, id.clone())
            && existing_id != id
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unique lookup `{}` key is used by multiple rows in one batch",
                    lookup.field_name()
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn sort_sixb_entries(rows: &mut [SixbRowEntry], lookups: &mut [SixbLookupEntry]) {
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    lookups.sort_by(|left, right| {
        left.field_name
            .cmp(&right.field_name)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.id.cmp(&right.id))
    });
}

pub(super) fn row_entry_by_id<'a>(cache: &'a SixbCache, id: &str) -> Option<&'a SixbRowEntry> {
    cache
        .rows
        .binary_search_by(|entry| entry.id.as_str().cmp(id))
        .ok()
        .map(|index| &cache.rows[index])
}

pub(super) fn lookup_entries_by_key<'a>(
    cache: &'a SixbCache,
    field_name: &str,
    key: &str,
) -> &'a [SixbLookupEntry] {
    let start = cache.lookups.partition_point(|entry| {
        (entry.field_name.as_str(), entry.key.as_str()) < (field_name, key)
    });
    let len = cache.lookups[start..]
        .partition_point(|entry| entry.field_name == field_name && entry.key == key);
    &cache.lookups[start..start + len]
}

pub(super) fn chunk_path(global_chunk_counter: u64) -> io::Result<PathBuf> {
    if global_chunk_counter >= MAX_CHUNKS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("chunk counter must be between 0 and {}", MAX_CHUNKS - 1),
        ));
    }

    let file = encode_reverse_base36(global_chunk_counter as usize, CHUNK_WIDTH)?;
    Ok(PathBuf::from(format!("{file}.6")))
}

pub(super) fn chunk_path_to_name(path: &Path) -> io::Result<String> {
    let value = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`", path.display()),
        )
    })?;
    Ok(value.replace('\\', "/"))
}

pub(super) fn encode_reverse_base36(n: usize, width: usize) -> io::Result<String> {
    let max = CHUNK_BASE.pow(width as u32);
    if n >= max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("value must be between 0 and {}", max - 1),
        ));
    }
    encode_fixed_base36(max - 1 - n, width)
}

pub(super) fn encode_fixed_base36(mut n: usize, width: usize) -> io::Result<String> {
    let max = CHUNK_BASE.pow(width as u32);
    if n >= max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("value must be between 0 and {}", max - 1),
        ));
    }

    let mut out = vec![b'0'; width];
    for i in (0..width).rev() {
        let digit = n % CHUNK_BASE;
        out[i] = CHUNK_CHARS[digit];
        n /= CHUNK_BASE;
    }
    String::from_utf8(out)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

pub(super) fn six_files_in_read_order(table_dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !table_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_six_files(table_dir, &mut files)?;
    files.sort();
    files.reverse();
    Ok(files)
}

pub(super) fn collect_six_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("6") {
            files.push(path);
        }
    }
    Ok(())
}

pub(super) fn relative_chunk_name(table_dir: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(table_dir).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`: {error}", path.display()),
        )
    })?;
    let value = relative.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad chunk path `{}`", relative.display()),
        )
    })?;
    Ok(value.replace('\\', "/"))
}

pub(super) fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn format_error_to_io(error: sixpack_format::FormatError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

pub(super) fn extend_source_hash(current: &str, bytes: &[u8]) -> io::Result<String> {
    let mut hash = if current.is_empty() {
        FNV_OFFSET_BASIS
    } else {
        u64::from_str_radix(current, 16).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad source hash `{current}`: {error}"),
            )
        })?
    };
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    Ok(format!("{hash:016x}"))
}
