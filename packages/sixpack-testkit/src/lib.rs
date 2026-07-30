//! Shared sixpack testing support.
//!
//! This crate is for reusable test harnesses, builders, assertions, and
//! compatibility checks used by workspace tests. It should not contain product
//! runtime logic.

use std::ops::Deref;

use sixpack::Database;
use tempfile::TempDir;

pub use sixpack::{DataProjection, write_data_projection};

/// Temporary database whose directory is deleted when the handle is dropped.
#[derive(Debug)]
pub struct TestDatabase {
    database: Database,
    _temp_dir: TempDir,
}

impl Deref for TestDatabase {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}

/// Creates a database handle backed by an automatically cleaned temporary directory.
pub fn test_database() -> std::io::Result<TestDatabase> {
    let temp_dir = tempfile::tempdir()?;
    let database = Database::open_local(temp_dir.path(), "test");
    Ok(TestDatabase {
        database,
        _temp_dir: temp_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn generated_projection_exists_for_a_new_empty_database() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("new-database");

        let result = write_data_projection(&database).unwrap();
        let html = fs::read_to_string(&result.path).unwrap();

        assert_eq!(result.path, database.join("projection.html"));
        assert_eq!(result.file_count, 0);
        assert_eq!(result.source_bytes, 0);
        assert!(html.contains(r#""files":[],"revision":"-""#));
        assert!(html.contains("search table"));
    }

    #[test]
    fn generated_projection_embeds_canonical_files_without_a_picker() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("demo");
        let table = database.join("tables").join("notes");
        fs::create_dir_all(database.join("engine")).unwrap();
        fs::create_dir_all(&table).unwrap();
        fs::write(database.join("engine").join("revision"), "7\n").unwrap();
        fs::write(
            table.join("zzz.6"),
            concat!(
                "SIX\t1\ttable\tnotes\t0123456789abcdef\n",
                "@field\tid\tid\n",
                "@field\ttitle\ttext\n",
                "@lookup\tid\tunique\n",
                "@data\n",
                "R\t1\tnote-1\tVisible immediately\n",
                "R\t2\tnote-2\t</script><script>alert('row')</script>\n",
            ),
        )
        .unwrap();

        let result = write_data_projection(&database).unwrap();
        let html = fs::read_to_string(&result.path).unwrap();
        assert_eq!(result.file_count, 1);
        assert!(html.contains("window.__SIXPACK_EMBEDDED__ = {"));
        assert!(html.contains("Visible immediately"));
        assert!(html.contains(r#""revision":"7""#));
        assert!(!html.contains("</script><script>alert('row')</script>"));
        assert!(html.contains(r"\u003c/script\u003e\u003cscript\u003e"));
        assert!(!html.contains("Open database"));
        assert!(!html.contains("Choose Files"));
    }
}
