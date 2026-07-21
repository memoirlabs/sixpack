use std::fs;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use rusqlite::{Connection, params};
use sixpack::{Database, DatabaseSchema, PrimitiveType, Record, TableSchema, Value};
use tempfile::TempDir;

const ROW_COUNTS: &[usize] = &[10_000, 100_000];
const INSERT_BATCH_SIZE: usize = 10_000;
const TABLE: &str = "events";

fn event_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut events = TableSchema::new(TABLE);
    events.add_field("id", PrimitiveType::Id).unwrap();
    events.add_field("stream_id", PrimitiveType::Id).unwrap();
    events.add_field("payload", PrimitiveType::Text).unwrap();
    events.add_lookup("stream_id", false).unwrap();
    schema.add_table(events).unwrap();
    schema
}

fn event_record(index: usize) -> Record {
    Record::new(TABLE)
        .with_id(format!("e{index:012}"))
        .unwrap()
        .with_field("stream_id", Value::Id(format!("s{}", index % 128)))
        .unwrap()
        .with_field(
            "payload",
            format!("event-{index:08}:abcdefghijklmnopqrstuvwxyz0123456789"),
        )
        .unwrap()
}

fn populated_database(rows: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_local_with_schema(dir.path(), "cache", event_schema());
    db.init().unwrap();

    for start in (0..rows).step_by(INSERT_BATCH_SIZE) {
        let end = (start + INSERT_BATCH_SIZE).min(rows);
        let records = (start..end).map(event_record).collect::<Vec<_>>();
        db.insert_many(&records).unwrap();
    }

    // Hot writes update the runtime projection lazily. Materialize a current
    // on-disk cache once so the benchmark isolates reopen behavior.
    db.rebuild_cache(TABLE).unwrap();
    assert!(dir.path().join("cache/engine/events.6b").is_file());
    dir
}

fn populated_sqlite(rows: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite3");
    let mut connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                stream_id TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX events_stream_id ON events(stream_id);
            ",
        )
        .unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut statement = transaction
            .prepare("INSERT INTO events (id, stream_id, payload) VALUES (?1, ?2, ?3)")
            .unwrap();
        for index in 0..rows {
            statement
                .execute(params![
                    format!("e{index:012}"),
                    format!("s{}", index % 128),
                    format!("event-{index:08}:abcdefghijklmnopqrstuvwxyz0123456789"),
                ])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    drop(connection);
    dir
}

fn bench_cache_reopen(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_reopen");

    for &rows in ROW_COUNTS {
        group.throughput(Throughput::Elements(rows as u64));
        let dir = populated_database(rows);
        let cache_path = dir.path().join("cache/engine/events.6b");
        let sqlite_dir = populated_sqlite(rows);
        let sqlite_path = sqlite_dir.path().join("cache.sqlite3");

        group.bench_with_input(
            BenchmarkId::new("cached_reopen_and_count", rows),
            &rows,
            |b, _| {
                b.iter(|| {
                    let db = Database::open_local_with_schema(dir.path(), "cache", event_schema());
                    black_box(db.count(TABLE).unwrap())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("missing_cache_rebuild_and_count", rows),
            &rows,
            |b, _| {
                b.iter_batched(
                    || {
                        if cache_path.exists() {
                            fs::remove_file(&cache_path).unwrap();
                        }
                    },
                    |()| {
                        let db =
                            Database::open_local_with_schema(dir.path(), "cache", event_schema());
                        black_box(db.count(TABLE).unwrap())
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sqlite_reopen_and_count", rows),
            &rows,
            |b, _| {
                b.iter(|| {
                    let connection = Connection::open(&sqlite_path).unwrap();
                    let count: i64 = connection
                        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
                        .unwrap();
                    black_box(count)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_cache_reopen
);
criterion_main!(benches);
