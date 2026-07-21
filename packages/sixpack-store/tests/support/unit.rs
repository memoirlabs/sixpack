use super::*;
use sixpack_core::{DatabaseSchema, PrimitiveType};
use std::collections::BTreeSet;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.push(format!(
        "sixpack-store-{name}-{}-{stamp}-{counter}",
        std::process::id()
    ));
    dir
}

fn schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages
        .add_field("created_at", PrimitiveType::Int)
        .unwrap();
    messages.add_lookup("created_at", false).unwrap();
    schema.add_table(messages).unwrap();
    schema
}

fn concurrent_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    for table_name in ["messages", "conversations"] {
        let mut table = TableSchema::new(table_name);
        table.add_field("id", PrimitiveType::Id).unwrap();
        table.add_field("body", PrimitiveType::Text).unwrap();
        schema.add_table(table).unwrap();
    }
    schema
}

fn simple_record(table: &str, id: String) -> Record {
    Record::new(table)
        .with_id(id.clone())
        .unwrap()
        .with_field("body", format!("body for {id}"))
        .unwrap()
}

fn message_record(id: &str, created_at: i64) -> Record {
    Record::new("messages")
        .with_id(id)
        .unwrap()
        .with_field("body", format!("body for {id}"))
        .unwrap()
        .with_field("created_at", created_at)
        .unwrap()
}

fn transaction_ids(store: &LocalStore, table: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    for path in six_files_in_read_order(&store.table_dir(table)).unwrap() {
        for line in fs::read_to_string(path).unwrap().lines() {
            if line.starts_with("R\t") || line.starts_with("D\t") {
                ids.push(line.split('\t').nth(1).unwrap().parse().unwrap());
            }
        }
    }
    ids
}

#[test]
fn separate_handles_refresh_after_each_others_writes() {
    let root = temp_root("separate-handles");
    let schema = concurrent_schema();
    let first = LocalStore::new(&root, "db");
    let second = LocalStore::new(&root, "db");
    first.init(&schema).unwrap();

    first
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    assert_eq!(second.count_table(&schema, "messages").unwrap(), 1);

    first
        .append_put(&schema, &simple_record("messages", "m2".to_owned()))
        .unwrap();
    assert_eq!(second.count_table(&schema, "messages").unwrap(), 2);
    assert!(
        second
            .get_by_id(&schema, "messages", "m2")
            .unwrap()
            .is_some()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn separate_handles_read_consistent_snapshots_while_another_writes() {
    let root = temp_root("read-while-write");
    let schema = Arc::new(concurrent_schema());
    let writer = LocalStore::new(&root, "db");
    let reader = LocalStore::new(&root, "db");
    writer.init(&schema).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let writer_thread = {
        let schema = Arc::clone(&schema);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            for index in 0..50 {
                writer
                    .append_put(&schema, &simple_record("messages", format!("m-{index:04}")))
                    .unwrap();
            }
        })
    };

    barrier.wait();
    let mut previous = 0;
    for _ in 0..10_000 {
        let count = reader.count_table(&schema, "messages").unwrap();
        assert!(count >= previous);
        let rows = reader.read_table(&schema, "messages").unwrap();
        assert!(rows.len() >= count);
        previous = count;
        if rows.len() == 50 {
            previous = rows.len();
            break;
        }
        thread::yield_now();
    }
    writer_thread.join().unwrap();
    assert_eq!(previous, 50);
    assert_eq!(reader.count_table(&schema, "messages").unwrap(), 50);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cloned_handle_serializes_cross_table_writes() {
    let root = temp_root("cross-table-threads");
    let schema = Arc::new(concurrent_schema());
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let workers = ["messages", "conversations"].map(|table| {
        let schema = Arc::clone(&schema);
        let store = store.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            (0..40)
                .map(|index| {
                    store
                        .append_put(
                            &schema,
                            &simple_record(table, format!("{table}-{index:04}")),
                        )
                        .unwrap()
                        .tx_id
                })
                .collect::<Vec<_>>()
        })
    });

    let tx_ids = workers
        .into_iter()
        .flat_map(|worker| worker.join().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(tx_ids.len(), 80);
    assert_eq!(store.count_table(&schema, "messages").unwrap(), 40);
    assert_eq!(store.count_table(&schema, "conversations").unwrap(), 40);

    let reopened = LocalStore::new(&root, "db");
    assert_eq!(reopened.count_table(&schema, "messages").unwrap(), 40);
    assert_eq!(reopened.count_table(&schema, "conversations").unwrap(), 40);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn dirty_revision_discards_an_incomplete_tail_before_the_next_write() {
    let root = temp_root("incomplete-tail");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();

    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let mut file = OpenOptions::new().append(true).open(&chunk).unwrap();
    file.write_all(b"R\t2\tm2\tpartial body").unwrap();
    file.sync_data().unwrap();
    fs::write(store.revision_path(), u64::MAX.to_string()).unwrap();

    let recovered = LocalStore::new(&root, "db");
    assert_eq!(recovered.count_table(&schema, "messages").unwrap(), 1);
    let result = recovered
        .append_put(&schema, &simple_record("messages", "m2".to_owned()))
        .unwrap();
    assert_eq!(result.tx_id, 2);
    assert_eq!(recovered.count_table(&schema, "messages").unwrap(), 2);
    let bytes = fs::read_to_string(chunk).unwrap();
    assert!(!bytes.contains("partial body"));

    let reopened = LocalStore::new(&root, "db");
    assert_eq!(reopened.count_table(&schema, "messages").unwrap(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn simulated_power_loss_during_append_recovers_all_query_shapes() {
    let root = temp_root("power-loss-tail");
    let schema = schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store.append_put(&schema, &message_record("m1", 1)).unwrap();

    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let mut file = OpenOptions::new().append(true).open(&chunk).unwrap();
    file.write_all(b"R\t2\tm2\tpartial body").unwrap();
    file.sync_data().unwrap();
    fs::write(store.revision_path(), u64::MAX.to_string()).unwrap();
    drop(file);
    drop(store);

    let recovered = LocalStore::new(&root, "db");
    assert_eq!(recovered.count_table(&schema, "messages").unwrap(), 1);
    assert!(
        recovered
            .get_by_id(&schema, "messages", "m1")
            .unwrap()
            .is_some()
    );
    assert!(
        recovered
            .get_by_id(&schema, "messages", "m2")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        recovered
            .get_by_lookup(&schema, "messages", "created_at", "1")
            .unwrap()
            .len(),
        1
    );

    recovered
        .append_put(&schema, &message_record("m2", 2))
        .unwrap();
    assert_eq!(recovered.count_table(&schema, "messages").unwrap(), 2);
    assert_eq!(
        recovered
            .get_by_lookup(&schema, "messages", "created_at", "2")
            .unwrap()
            .len(),
        1
    );
    assert!(!fs::read_to_string(&chunk).unwrap().contains("partial body"));
    drop(recovered);

    let restarted = LocalStore::new(&root, "db");
    assert_eq!(restarted.count_table(&schema, "messages").unwrap(), 2);
    assert!(
        restarted
            .get_by_id(&schema, "messages", "m2")
            .unwrap()
            .is_some()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn abrupt_exit_after_synced_commit_is_queryable_after_restart() {
    let root = temp_root("abrupt-exit-after-commit");
    let schema = schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    drop(store);

    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("store::tests::abrupt_exit_after_synced_commit_child")
        .env("SIXPACK_ABRUPT_EXIT_ROOT", &root)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(99));

    let restarted = LocalStore::new(&root, "db");
    assert_eq!(restarted.count_table(&schema, "messages").unwrap(), 1);
    assert!(
        restarted
            .get_by_id(&schema, "messages", "m-power")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        restarted
            .get_by_lookup(&schema, "messages", "created_at", "42")
            .unwrap()
            .len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn abrupt_exit_after_synced_commit_child() {
    let Ok(root) = std::env::var("SIXPACK_ABRUPT_EXIT_ROOT") else {
        return;
    };
    let schema = schema();
    let store = LocalStore::new(root, "db");
    store
        .append_put(&schema, &message_record("m-power", 42))
        .unwrap();
    std::process::exit(99);
}

#[test]
fn dirty_revision_recovers_an_incomplete_tail_before_a_cross_table_write() {
    let root = temp_root("cross-table-recovery");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();

    let message_chunk = store.chunk_six_path("messages", 0).unwrap();
    let mut file = OpenOptions::new()
        .append(true)
        .open(&message_chunk)
        .unwrap();
    file.write_all(b"R\t2\tm2\tpartial body").unwrap();
    file.sync_data().unwrap();
    fs::write(store.revision_path(), u64::MAX.to_string()).unwrap();

    let recovered = LocalStore::new(&root, "db");
    let result = recovered
        .append_put(&schema, &simple_record("conversations", "c1".to_owned()))
        .unwrap();
    assert_eq!(result.tx_id, 2);
    assert_eq!(recovered.current_revision().unwrap(), 3);
    assert_eq!(recovered.count_table(&schema, "messages").unwrap(), 1);
    assert_eq!(recovered.count_table(&schema, "conversations").unwrap(), 1);
    assert!(
        !fs::read_to_string(message_chunk)
            .unwrap()
            .contains("partial body")
    );

    let cold = LocalStore::new(&root, "db");
    assert_eq!(cold.count_table(&schema, "messages").unwrap(), 1);
    assert_eq!(cold.count_table(&schema, "conversations").unwrap(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clean_workspace_rejects_an_unexpected_incomplete_tail_without_mutating_it() {
    let root = temp_root("clean-incomplete-tail");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();

    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let mut file = OpenOptions::new().append(true).open(&chunk).unwrap();
    file.write_all(b"unexpected partial data").unwrap();
    file.sync_data().unwrap();
    let before = fs::read(&chunk).unwrap();
    let revision_before = fs::read_to_string(store.revision_path()).unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold
        .append_put(&schema, &simple_record("messages", "m2".to_owned()))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("incomplete .6 tail"));
    assert_eq!(fs::read(&chunk).unwrap(), before);
    assert_eq!(
        fs::read_to_string(store.revision_path()).unwrap(),
        revision_before
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_generated_cache_is_rebuilt_from_canonical_rows() {
    let root = temp_root("corrupt-sixb");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    fs::write(store.sixb_path("messages"), b"not a sixb cache").unwrap();

    let cold = LocalStore::new(&root, "db");
    let row = cold.get_by_id(&schema, "messages", "m1").unwrap().unwrap();
    assert_eq!(record_id(&row).unwrap(), "m1");
    assert!(decode_sixb_cache(&fs::read(store.sixb_path("messages")).unwrap()).is_ok());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_complete_canonical_row_fails_loudly() {
    let root = temp_root("corrupt-canonical-row");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let mut file = OpenOptions::new().append(true).open(&chunk).unwrap();
    file.write_all(b"R\t2\tmissing-body-column\n").unwrap();
    file.sync_data().unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold.count_table(&schema, "messages").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("column count"));
    assert!(
        fs::read_to_string(chunk)
            .unwrap()
            .contains("missing-body-column")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mismatched_canonical_header_fails_loudly() {
    let root = temp_root("mismatched-header");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let contents = fs::read_to_string(&chunk).unwrap();
    let (_, body) = contents.split_once('\n').unwrap();
    fs::write(&chunk, format!("SIX\t1\ttable\tother\tdeadbeef\n{body}")).unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold.count_table(&schema, "messages").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("expected .6 header"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_revision_marker_blocks_access_without_touching_canonical_data() {
    let root = temp_root("corrupt-revision");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let before = fs::read(&chunk).unwrap();
    fs::write(store.revision_path(), b"not-a-revision").unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold.count_table(&schema, "messages").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("bad workspace revision"));
    assert_eq!(fs::read(chunk).unwrap(), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_metadata_recovers_transaction_ids_from_canonical_rows() {
    let root = temp_root("missing-metadata");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    for index in 1..=3 {
        store
            .append_put(&schema, &simple_record("messages", format!("m{index}")))
            .unwrap();
    }
    fs::remove_file(store.metadata_path()).unwrap();

    let cold = LocalStore::new(&root, "db");
    let result = cold
        .append_put(&schema, &simple_record("messages", "m4".to_owned()))
        .unwrap();
    assert_eq!(result.tx_id, 4);
    assert_eq!(cold.count_table(&schema, "messages").unwrap(), 4);
    assert!(cold.metadata_path().exists());
    assert_eq!(LocalStore::new(&root, "db").next_tx_id().unwrap(), 5);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_metadata_counter_blocks_a_write_without_appending_a_row() {
    let root = temp_root("corrupt-metadata");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    store
        .append_put(&schema, &simple_record("messages", "m1".to_owned()))
        .unwrap();
    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let before = fs::read(&chunk).unwrap();
    let metadata = fs::read_to_string(store.metadata_path()).unwrap();
    fs::write(
        store.metadata_path(),
        metadata.replace("next_tx = 1", "next_tx = broken"),
    )
    .unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold
        .append_put(&schema, &simple_record("messages", "m2".to_owned()))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("bad next_tx"));
    assert_eq!(fs::read(chunk).unwrap(), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_process_writes_are_serialized_and_recoverable() {
    let root = temp_root("two-processes");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    let start = root.join("start");
    let executable = std::env::current_exe().unwrap();

    let mut children = ["messages", "conversations"].map(|table| {
        Command::new(&executable)
            .arg("--exact")
            .arg("store::tests::two_process_writer_child")
            .env("SIXPACK_PROCESS_TEST_ROOT", &root)
            .env("SIXPACK_PROCESS_TEST_TABLE", table)
            .spawn()
            .unwrap()
    });
    fs::write(&start, b"go").unwrap();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }

    assert_eq!(store.count_table(&schema, "messages").unwrap(), 40);
    assert_eq!(store.count_table(&schema, "conversations").unwrap(), 40);
    let tx_ids = ["messages", "conversations"]
        .into_iter()
        .flat_map(|table| transaction_ids(&store, table))
        .collect::<BTreeSet<_>>();
    assert_eq!(tx_ids.len(), 80);
    assert_eq!(tx_ids.first(), Some(&1));
    assert_eq!(tx_ids.last(), Some(&80));
    assert_eq!(store.current_revision().unwrap(), 81);

    let reopened = LocalStore::new(&root, "db");
    assert_eq!(reopened.count_table(&schema, "messages").unwrap(), 40);
    assert_eq!(reopened.count_table(&schema, "conversations").unwrap(), 40);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_processes_can_write_the_same_chat_table_without_lost_rows() {
    let root = temp_root("two-processes-same-table");
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    store.init(&schema).unwrap();
    let start = root.join("start");
    let executable = std::env::current_exe().unwrap();

    let mut children = ["agent-a", "agent-b"].map(|prefix| {
        Command::new(&executable)
            .arg("--exact")
            .arg("store::tests::two_process_writer_child")
            .env("SIXPACK_PROCESS_TEST_ROOT", &root)
            .env("SIXPACK_PROCESS_TEST_TABLE", "messages")
            .env("SIXPACK_PROCESS_TEST_PREFIX", prefix)
            .spawn()
            .unwrap()
    });
    fs::write(&start, b"go").unwrap();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }

    assert_eq!(store.count_table(&schema, "messages").unwrap(), 80);
    let tx_ids = transaction_ids(&store, "messages")
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(tx_ids.len(), 80);
    assert_eq!(tx_ids.first(), Some(&1));
    assert_eq!(tx_ids.last(), Some(&80));
    for prefix in ["agent-a", "agent-b"] {
        for index in 0..40 {
            let id = format!("{prefix}-{index:04}");
            assert!(store.get_by_id(&schema, "messages", &id).unwrap().is_some());
        }
    }

    let cold = LocalStore::new(&root, "db");
    assert_eq!(cold.count_table(&schema, "messages").unwrap(), 80);
    assert_eq!(cold.current_revision().unwrap(), 81);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_process_writer_child() {
    let Ok(root) = std::env::var("SIXPACK_PROCESS_TEST_ROOT") else {
        return;
    };
    let table = std::env::var("SIXPACK_PROCESS_TEST_TABLE").unwrap();
    let prefix = std::env::var("SIXPACK_PROCESS_TEST_PREFIX").unwrap_or_else(|_| table.clone());
    let root = PathBuf::from(root);
    while !root.join("start").exists() {
        thread::yield_now();
    }
    let schema = concurrent_schema();
    let store = LocalStore::new(&root, "db");
    for index in 0..40 {
        store
            .append_put(
                &schema,
                &simple_record(&table, format!("{prefix}-{index:04}")),
            )
            .unwrap();
    }
}

#[test]
fn append_writes_six_layout_and_metadata() {
    let root = temp_root("six");
    let schema = schema();
    let store = LocalStore::new(&root, "db");
    let first = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello\tworld")
        .unwrap()
        .with_field("created_at", 1i64)
        .unwrap();

    let second = Record::new("messages")
        .with_id("m2")
        .unwrap()
        .with_field("body", "line\nbreak")
        .unwrap()
        .with_field("created_at", 2i64)
        .unwrap();

    let one = store.append_put(&schema, &first).unwrap();
    let two = store.append_put(&schema, &second).unwrap();
    assert_eq!(one.tx_id, 1);
    assert_eq!(two.tx_id, 2);
    assert_eq!(one.operation, Operation::Put);
    assert!(one.bytes_written > 0);

    let chunk = fs::read_to_string(store.chunk_six_path("messages", 0).unwrap()).unwrap();
    assert!(!store.chunk_six_path("messages", 1).unwrap().exists());
    assert!(chunk.starts_with("SIX\t1\ttable\tmessages\t"));
    assert!(chunk.contains("@field\tid\tid\n"));
    assert!(chunk.contains("@field\tbody\ttext\n"));
    assert!(chunk.contains("@lookup\tid\tunique\n"));
    assert!(chunk.contains("@lookup\tcreated_at\tmany\n"));
    assert!(chunk.contains("@data\n"));
    assert!(chunk.contains("R\t1\tm1\thello\\tworld\t1\n"));
    assert!(chunk.contains("R\t2\tm2\tline\\nbreak\t2\n"));
    assert!(!chunk.contains("\tput\t"));

    let metadata = fs::read_to_string(store.metadata_path()).unwrap();
    assert!(metadata.contains("[tables.messages]"));
    assert!(metadata.contains("next_tx = 2"));
    assert!(metadata.contains("next_chunk = 1"));
    assert!(!metadata.contains("chunks = ["));
    assert!(metadata.contains("file = \"engine/messages.6b\""));
    assert!(store.sixb_path("messages").exists());

    let rows = store.read_table(&schema, "messages").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fields().get("id"), first.fields().get("id"));
    assert_eq!(rows[1].fields().get("id"), second.fields().get("id"));

    let incremental_cache = store.ensure_sixb_current(&schema, "messages").unwrap();
    let rebuilt_cache = store.rebuild_sixb(&schema, "messages").unwrap();
    assert_eq!(incremental_cache, rebuilt_cache);

    let recovered = LocalStore::new(&root, "db");
    assert_eq!(recovered.next_tx_id().unwrap(), 3);
    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "experimental-compaction")]
#[test]
fn compact_table_rewrites_live_rows_and_rebuilds_cache() {
    let root = temp_root("compact");
    let schema = schema();
    let store = LocalStore::new(&root, "db");

    let original = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "first")
        .unwrap()
        .with_field("created_at", 1i64)
        .unwrap();
    store.append_put(&schema, &original).unwrap();
    for index in 0..25 {
        let updated = Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", format!("update {index}"))
            .unwrap()
            .with_field("created_at", i64::from(index + 2))
            .unwrap();
        store.append_put(&schema, &updated).unwrap();
    }

    let before = fs::metadata(store.chunk_six_path("messages", 0).unwrap())
        .unwrap()
        .len();
    let result = store.compact_table(&schema, "messages").unwrap();

    assert_eq!(result.table, "messages");
    assert_eq!(result.live_rows, 1);
    assert_eq!(result.chunks_before, 1);
    assert_eq!(result.chunks_after, 1);
    assert_eq!(result.bytes_before, before);
    assert!(result.bytes_after < result.bytes_before);
    assert_eq!(result.chunk_name, "zzz.6");
    assert!(store.chunk_six_path("messages", 0).unwrap().exists());
    assert!(!store.chunk_six_path("messages", 1).unwrap().exists());
    assert_eq!(store.next_chunk_counter("messages").unwrap(), 1);

    let chunk = fs::read_to_string(store.chunk_six_path("messages", 0).unwrap()).unwrap();
    assert!(chunk.starts_with("SIX\t1\ttable\tmessages\t"));
    assert!(chunk.contains("R\t27\tm1\tupdate 24\t26\n"));
    assert!(!chunk.contains("\tfirst\t"));
    assert!(!chunk.contains("\tupdate 0\t"));

    let row = store.get_by_id(&schema, "messages", "m1").unwrap().unwrap();
    assert_eq!(
        row.fields().get("body"),
        Some(&Value::Text("update 24".to_owned()))
    );
    let cache = store.ensure_sixb_current(&schema, "messages").unwrap();
    assert_eq!(cache.rows.len(), 1);
    assert_eq!(cache.rows[0].ptr.chunk_name, "zzz.6");

    let metadata = fs::read_to_string(store.metadata_path()).unwrap();
    assert!(metadata.contains("next_tx = 28"));
    assert!(metadata.contains("next_chunk = 1"));
    assert!(metadata.contains("file = \"engine/messages.6b\""));

    let reopened = LocalStore::new(&root, "db");
    let row = reopened
        .get_by_id(&schema, "messages", "m1")
        .unwrap()
        .unwrap();
    assert_eq!(
        row.fields().get("body"),
        Some(&Value::Text("update 24".to_owned()))
    );
    assert_eq!(reopened.next_tx_id().unwrap(), 28);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn chunk_paths_reverse_sort_from_counter() {
    assert_eq!(chunk_path(0).unwrap(), PathBuf::from("zzz.6"));
    assert_eq!(chunk_path(1).unwrap(), PathBuf::from("zzy.6"));
    assert_eq!(chunk_path(2).unwrap(), PathBuf::from("zzx.6"));
    assert_eq!(chunk_path(46_655).unwrap(), PathBuf::from("000.6"));
    assert!(chunk_path(46_656).is_err());
}

#[test]
fn direct_store_writes_still_enforce_schema_types() {
    let root = temp_root("direct-store-validation");
    let schema = schema();
    let store = LocalStore::new(&root, "db");
    let invalid = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "body")
        .unwrap()
        .with_field("created_at", "not-an-int")
        .unwrap();

    let error = store.append_put(&schema, &invalid).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(store.count_table(&schema, "messages").unwrap(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn canonical_preamble_requires_the_exact_table_shape() {
    let root = temp_root("table-shape-mismatch");
    let schema = schema();
    let store = LocalStore::new(&root, "db");
    store.append_put(&schema, &message_record("m1", 1)).unwrap();
    let chunk = store.chunk_six_path("messages", 0).unwrap();
    let contents = fs::read_to_string(&chunk).unwrap();
    let rewritten = contents.replace("@field\tbody\ttext", "@field\tbody\tint");
    fs::write(&chunk, rewritten).unwrap();

    let cold = LocalStore::new(&root, "db");
    let error = cold.count_table(&schema, "messages").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("header/preamble"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn generated_index_kinds_have_stable_distinct_extensions() {
    let root = temp_root("index-paths");
    let store = LocalStore::new(&root, "db");
    assert_eq!(
        store.generated_index_path("messages", GeneratedIndexKind::Lookup),
        store.sixb_path("messages")
    );
    assert_eq!(
        store.generated_index_path("messages", GeneratedIndexKind::FullText),
        store.sixx_path("messages")
    );
    assert!(store.sixb_path("messages").ends_with("messages.6b"));
    assert!(store.sixx_path("messages").ends_with("messages.6x"));
}
