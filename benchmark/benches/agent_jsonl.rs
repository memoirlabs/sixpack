use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use serde_json::{Value as JsonValue, json};
use sixpack::{
    Database, DatabaseSchema, PrimitiveType, Record, TableSchema, Value, change, selector,
};
use tempfile::TempDir;

const CONVERSATIONS: usize = 100;
const MESSAGES_PER_CONVERSATION: usize = 100;
const TOTAL_MESSAGES: usize = CONVERSATIONS * MESSAGES_PER_CONVERSATION;
const TABLE: &str = "messages";
const TARGET_CONVERSATION: usize = 42;

fn message_schema() -> DatabaseSchema {
    let mut schema = DatabaseSchema::new();
    let mut messages = TableSchema::new(TABLE);
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages
        .add_field("conversation_id", PrimitiveType::Id)
        .unwrap();
    messages.add_field("role", PrimitiveType::Text).unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages.add_field("status", PrimitiveType::Text).unwrap();
    messages.add_field("sequence", PrimitiveType::Int).unwrap();
    messages.add_lookup("conversation_id", false).unwrap();
    messages.add_lookup("status", false).unwrap();
    schema.add_table(messages).unwrap();
    schema
}

fn conversation_id(conversation: usize) -> String {
    format!("c{conversation:04}")
}

fn message_id(conversation: usize, sequence: usize) -> String {
    format!("c{conversation:04}-m{sequence:05}")
}

fn message_status(conversation: usize, sequence: usize) -> &'static str {
    if (conversation * MESSAGES_PER_CONVERSATION + sequence).is_multiple_of(1_000) {
        "streaming"
    } else {
        "completed"
    }
}

fn message_body(conversation: usize, sequence: usize) -> String {
    format!(
        "conversation {conversation:04} message {sequence:05}: \
         abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    )
}

fn message_record(conversation: usize, sequence: usize) -> Record {
    Record::new(TABLE)
        .with_id(message_id(conversation, sequence))
        .unwrap()
        .with_field("conversation_id", Value::Id(conversation_id(conversation)))
        .unwrap()
        .with_field(
            "role",
            if sequence.is_multiple_of(2) {
                "user"
            } else {
                "assistant"
            },
        )
        .unwrap()
        .with_field("body", message_body(conversation, sequence))
        .unwrap()
        .with_field("status", message_status(conversation, sequence))
        .unwrap()
        .with_field("sequence", sequence as i64)
        .unwrap()
}

fn jsonl_message(conversation: usize, sequence: usize) -> String {
    json!({
        "id": message_id(conversation, sequence),
        "conversation_id": conversation_id(conversation),
        "role": if sequence.is_multiple_of(2) { "user" } else { "assistant" },
        "body": message_body(conversation, sequence),
        "status": message_status(conversation, sequence),
        "sequence": sequence,
    })
    .to_string()
}

fn populated_sixpack() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_local_with_schema(dir.path(), "agent", message_schema());
    db.init().unwrap();
    let records = (0..CONVERSATIONS)
        .flat_map(|conversation| {
            (0..MESSAGES_PER_CONVERSATION)
                .map(move |sequence| message_record(conversation, sequence))
        })
        .collect::<Vec<_>>();
    db.insert_many(&records).unwrap();
    db.rebuild_cache(TABLE).unwrap();
    (dir, db)
}

fn populated_jsonl() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for conversation in 0..CONVERSATIONS {
        let file = File::create(jsonl_path(dir.path(), conversation)).unwrap();
        let mut writer = BufWriter::new(file);
        for sequence in 0..MESSAGES_PER_CONVERSATION {
            writeln!(writer, "{}", jsonl_message(conversation, sequence)).unwrap();
        }
        writer.flush().unwrap();
        writer.get_ref().sync_data().unwrap();
    }
    dir
}

fn jsonl_path(root: &Path, conversation: usize) -> PathBuf {
    root.join(format!("{}.jsonl", conversation_id(conversation)))
}

fn read_jsonl_history(path: &Path) -> Vec<JsonValue> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn find_jsonl_message(path: &Path, target_id: &str) -> Option<JsonValue> {
    fs::read_to_string(path).unwrap().lines().find_map(|line| {
        let value = serde_json::from_str::<JsonValue>(line).unwrap();
        (value["id"] == target_id).then_some(value)
    })
}

fn scan_jsonl_status(paths: &[PathBuf], status: &str) -> Vec<JsonValue> {
    paths
        .iter()
        .flat_map(|path| read_jsonl_history(path))
        .filter(|message| message["status"] == status)
        .collect()
}

fn bench_agent_jsonl(c: &mut Criterion) {
    let (sixpack_dir, sixpack) = populated_sixpack();
    let jsonl_dir = populated_jsonl();
    let target_conversation = conversation_id(TARGET_CONVERSATION);
    let target_jsonl = jsonl_path(jsonl_dir.path(), TARGET_CONVERSATION);
    let jsonl_paths = (0..CONVERSATIONS)
        .map(|conversation| jsonl_path(jsonl_dir.path(), conversation))
        .collect::<Vec<_>>();

    let mut history = c.benchmark_group("agent_history_100_of_10k_messages");
    history.throughput(Throughput::Elements(MESSAGES_PER_CONVERSATION as u64));
    history.bench_function("sixpack_declared_lookup", |b| {
        b.iter(|| {
            black_box(
                sixpack
                    .get_many_by(TABLE, "conversation_id", &target_conversation)
                    .unwrap(),
            )
        });
    });
    history.bench_function("jsonl_read_conversation_file", |b| {
        b.iter(|| black_box(read_jsonl_history(&target_jsonl)));
    });
    history.finish();

    let target_id = message_id(TARGET_CONVERSATION, 50);
    let mut by_id = c.benchmark_group("agent_get_message_by_id_10k_messages");
    by_id.bench_function("sixpack_id_index", |b| {
        b.iter(|| black_box(sixpack.get(selector::id(TABLE, target_id.clone())).unwrap()));
    });
    by_id.bench_function("jsonl_scan_known_conversation", |b| {
        b.iter(|| black_box(find_jsonl_message(&target_jsonl, &target_id)));
    });
    by_id.finish();

    let mut status = c.benchmark_group("agent_global_status_query_10k_messages");
    status.throughput(Throughput::Elements(TOTAL_MESSAGES as u64));
    status.bench_function("sixpack_declared_lookup", |b| {
        b.iter(|| black_box(sixpack.get_many_by(TABLE, "status", "streaming").unwrap()));
    });
    status.bench_function("jsonl_scan_all_conversations", |b| {
        b.iter(|| black_box(scan_jsonl_status(&jsonl_paths, "streaming")));
    });
    status.finish();

    let mut reopen = c.benchmark_group("agent_reopen_and_read_history_10k_messages");
    reopen.throughput(Throughput::Elements(MESSAGES_PER_CONVERSATION as u64));
    reopen.bench_function("sixpack_cached_reopen_lookup", |b| {
        b.iter(|| {
            let db =
                Database::open_local_with_schema(sixpack_dir.path(), "agent", message_schema());
            black_box(
                db.get_many_by(TABLE, "conversation_id", &target_conversation)
                    .unwrap(),
            )
        });
    });
    reopen.bench_function("jsonl_open_conversation_file", |b| {
        b.iter(|| black_box(read_jsonl_history(&target_jsonl)));
    });
    reopen.finish();

    let mut append = c.benchmark_group("agent_durable_single_message_append");
    append.throughput(Throughput::Elements(1));
    let sixpack_next = AtomicUsize::new(MESSAGES_PER_CONVERSATION);
    append.bench_function("sixpack_synced_write", |b| {
        b.iter(|| {
            let sequence = sixpack_next.fetch_add(1, Ordering::Relaxed);
            black_box(
                sixpack
                    .write(change::add(message_record(0, sequence)))
                    .unwrap(),
            )
        });
    });

    let jsonl_next = AtomicUsize::new(MESSAGES_PER_CONVERSATION);
    let append_path = jsonl_path(jsonl_dir.path(), 0);
    append.bench_function("jsonl_append_and_sync_data", |b| {
        b.iter(|| {
            let sequence = jsonl_next.fetch_add(1, Ordering::Relaxed);
            let mut file = OpenOptions::new().append(true).open(&append_path).unwrap();
            writeln!(file, "{}", jsonl_message(0, sequence)).unwrap();
            file.sync_data().unwrap();
            black_box(sequence)
        });
    });
    append.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_agent_jsonl
);
criterion_main!(benches);
