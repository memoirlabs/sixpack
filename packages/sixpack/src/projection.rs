use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io};

const TEMPLATE: &str = include_str!("../assets/data-projection.html");
const MARKER: &str = "window.__SIXPACK_EMBEDDED__ = null;";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generated, self-contained HTML projection for a local database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataProjection {
    pub path: PathBuf,
    pub file_count: usize,
    pub source_bytes: u64,
}

/// Writes a read-only `projection.html` beside a local database.
///
/// Canonical `.6` files are embedded so the result opens directly without a
/// server, file picker, or browser filesystem permission.
pub fn write_data_projection(database_dir: &Path) -> io::Result<DataProjection> {
    let tables_dir = database_dir.join("tables");
    let mut table_dirs = if tables_dir.exists() {
        fs::read_dir(&tables_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    table_dirs.retain(|path| path.is_dir());
    table_dirs.sort();

    let mut files = Vec::new();
    let mut source_bytes = 0u64;
    for table_dir in table_dirs {
        let table_name = table_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad table directory"))?;
        let mut chunks = fs::read_dir(&table_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        chunks.retain(|path| path.extension().and_then(|value| value.to_str()) == Some("6"));
        chunks.sort();
        for chunk in chunks {
            let file_name = chunk
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad chunk path"))?;
            let source = fs::read_to_string(&chunk)?;
            let size = u64::try_from(source.len())
                .map_err(|_| io::Error::other("projection source is too large"))?;
            source_bytes = source_bytes.saturating_add(size);
            files.push(serde_json::json!({
                "path": format!("tables/{table_name}/{file_name}"),
                "tableHint": table_name,
                "source": source,
                "size": size,
            }));
        }
    }

    let revision = fs::read_to_string(database_dir.join("engine").join("revision"))
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|_| "-".to_owned());
    let payload = serde_json::json!({
        "database": database_dir.display().to_string(),
        "revision": revision,
        "files": files,
    });
    let embedded = inline_script_json(&payload)?;
    let html = TEMPLATE.replace(
        MARKER,
        &format!("window.__SIXPACK_EMBEDDED__ = {embedded};"),
    );
    if html == TEMPLATE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "data projection template marker is missing",
        ));
    }

    fs::create_dir_all(database_dir)?;
    let path = database_dir.join("projection.html");
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = database_dir.join(format!(
        "projection.html.{}.{counter}.tmp",
        std::process::id()
    ));
    fs::write(&temporary, html)?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(DataProjection {
        path,
        file_count: payload["files"].as_array().map_or(0, Vec::len),
        source_bytes,
    })
}

fn inline_script_json(value: &serde_json::Value) -> io::Result<String> {
    Ok(serde_json::to_string(value)
        .map_err(io::Error::other)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}
