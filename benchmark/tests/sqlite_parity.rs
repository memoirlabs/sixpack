use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sixpack::{
    Database, DatabaseSchema, PrimitiveType, Record, TableSchema, Value, change, selector,
};

const TABLE: &str = "events";

#[derive(Debug, PartialEq, Eq)]
struct Event {
    id: String,
    stream_id: String,
    payload: String,
    score: i64,
}

fn event_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut events = TableSchema::new(TABLE);
    events.add_field("id", PrimitiveType::Id).unwrap();
    events.add_field("stream_id", PrimitiveType::Id).unwrap();
    events.add_field("payload", PrimitiveType::Text).unwrap();
    events.add_field("score", PrimitiveType::Int).unwrap();
    events.add_lookup("stream_id", false).unwrap();
    schema.add_table(events).unwrap();
    schema
}

fn event_record(index: usize) -> Record {
    Record::new(TABLE)
        .with_id(format!("e{index:04}"))
        .unwrap()
        .with_field("stream_id", Value::Id(format!("s{}", index % 8)))
        .unwrap()
        .with_field("payload", format!("payload {index}"))
        .unwrap()
        .with_field("score", index as i64)
        .unwrap()
}

fn event_from_record(record: Record) -> Event {
    let fields = record.fields();
    Event {
        id: match fields.get("id").unwrap() {
            Value::Id(value) => value.clone(),
            value => panic!("expected id, got {value:?}"),
        },
        stream_id: match fields.get("stream_id").unwrap() {
            Value::Id(value) => value.clone(),
            value => panic!("expected stream id, got {value:?}"),
        },
        payload: match fields.get("payload").unwrap() {
            Value::Text(value) => value.clone(),
            value => panic!("expected payload, got {value:?}"),
        },
        score: match fields.get("score").unwrap() {
            Value::Int(value) => *value,
            value => panic!("expected score, got {value:?}"),
        },
    }
}

fn sqlite_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        stream_id: row.get(1)?,
        payload: row.get(2)?,
        score: row.get(3)?,
    })
}

fn open_sqlite(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn assert_query_parity(db: &Database, sqlite: &Connection) {
    let sixpack_count = db.count(TABLE).unwrap();
    let sqlite_count: usize = sqlite
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sixpack_count, sqlite_count);

    let sixpack_id = db
        .get(selector::id(TABLE, "e0128"))
        .unwrap()
        .map(event_from_record);
    let sqlite_id = sqlite
        .query_row(
            "SELECT id, stream_id, payload, score FROM events WHERE id = ?1",
            ["e0128"],
            sqlite_event,
        )
        .optional()
        .unwrap();
    assert_eq!(sixpack_id, sqlite_id);

    let sixpack_lookup = db
        .get_many_by(TABLE, "stream_id", "s0")
        .unwrap()
        .into_iter()
        .map(event_from_record)
        .collect::<Vec<_>>();
    let mut statement = sqlite
        .prepare(
            "SELECT id, stream_id, payload, score FROM events \
             WHERE stream_id = ?1 ORDER BY id",
        )
        .unwrap();
    let sqlite_lookup = statement
        .query_map(["s0"], sqlite_event)
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(sixpack_lookup, sqlite_lookup);

    let first_page = db.get_page_by(TABLE, "stream_id", "s0", 7, None).unwrap();
    let second_page = db
        .get_page_by(
            TABLE,
            "stream_id",
            "s0",
            7,
            first_page.next_cursor.as_deref(),
        )
        .unwrap();
    let sixpack_pages = first_page
        .rows
        .into_iter()
        .chain(second_page.rows)
        .map(event_from_record)
        .collect::<Vec<_>>();
    let sqlite_pages = sqlite
        .prepare(
            "SELECT id, stream_id, payload, score FROM events \
             WHERE stream_id = ?1 ORDER BY id LIMIT 14",
        )
        .unwrap()
        .query_map(["s0"], sqlite_event)
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(sixpack_pages, sqlite_pages);

    let sixpack_scan = db
        .scan(TABLE, Some(25), None)
        .unwrap()
        .rows
        .into_iter()
        .map(event_from_record)
        .collect::<Vec<_>>();
    let sqlite_scan = sqlite
        .prepare("SELECT id, stream_id, payload, score FROM events ORDER BY id LIMIT 25")
        .unwrap()
        .query_map([], sqlite_event)
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(sixpack_scan, sqlite_scan);
}

#[test]
fn sqlite_and_sixpack_queries_match_across_restart_and_cache_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let sqlite_path = temp.path().join("events.sqlite3");
    let mut sqlite = open_sqlite(&sqlite_path);
    sqlite
        .execute_batch(
            "
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                stream_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                score INTEGER NOT NULL
            );
            CREATE INDEX events_stream_id ON events(stream_id);
            ",
        )
        .unwrap();

    let schema = event_schema();
    let db = Database::open_local_with_schema(temp.path(), "sixpack", schema.clone());
    db.init().unwrap();
    let records = (0..200).map(event_record).collect::<Vec<_>>();
    db.insert_many(&records).unwrap();

    let transaction = sqlite.transaction().unwrap();
    {
        let mut statement = transaction
            .prepare("INSERT INTO events (id, stream_id, payload, score) VALUES (?1, ?2, ?3, ?4)")
            .unwrap();
        for index in 0..200 {
            statement
                .execute(params![
                    format!("e{index:04}"),
                    format!("s{}", index % 8),
                    format!("payload {index}"),
                    index as i64,
                ])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    assert_query_parity(&db, &sqlite);

    let edits = (10..20)
        .map(|index| {
            change::edit_id(
                TABLE,
                format!("e{index:04}"),
                BTreeMap::from([
                    (
                        "payload".to_owned(),
                        Value::Text(format!("updated {index}")),
                    ),
                    ("score".to_owned(), Value::Int((index + 1_000) as i64)),
                ]),
            )
        })
        .collect::<Vec<_>>();
    db.write_many(&edits).unwrap();
    let transaction = sqlite.transaction().unwrap();
    for index in 10..20 {
        transaction
            .execute(
                "UPDATE events SET payload = ?1, score = ?2 WHERE id = ?3",
                params![
                    format!("updated {index}"),
                    (index + 1_000) as i64,
                    format!("e{index:04}"),
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();

    let removes = (30..35)
        .map(|index| change::remove_id(TABLE, format!("e{index:04}")))
        .collect::<Vec<_>>();
    db.write_many(&removes).unwrap();
    let transaction = sqlite.transaction().unwrap();
    for index in 30..35 {
        transaction
            .execute("DELETE FROM events WHERE id = ?1", [format!("e{index:04}")])
            .unwrap();
    }
    transaction.commit().unwrap();
    assert_query_parity(&db, &sqlite);

    drop(db);
    drop(sqlite);
    let reopened = Database::open_local_with_schema(temp.path(), "sixpack", schema.clone());
    let sqlite = open_sqlite(&sqlite_path);
    assert_query_parity(&reopened, &sqlite);
    drop(reopened);

    let cache_path = temp.path().join("sixpack/engine/events.6b");
    fs::remove_file(&cache_path).unwrap();
    let rebuilt = Database::open_local_with_schema(temp.path(), "sixpack", schema);
    assert_query_parity(&rebuilt, &sqlite);
    assert!(cache_path.is_file());
}
