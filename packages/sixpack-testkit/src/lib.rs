//! Shared sixpack testing support.
//!
//! This crate is for reusable test harnesses, builders, assertions, and
//! compatibility checks used by workspace tests. It should not contain product
//! runtime logic.

use std::ops::Deref;

use sixpack::Database;
use tempfile::TempDir;

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
