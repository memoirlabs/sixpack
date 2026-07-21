use sixpack::{schema, table, *};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let mut dir = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.push(format!(
        "sixpack-db-{}-{stamp}-{counter}",
        std::process::id()
    ));
    dir
}

fn record_id(record: &Record) -> Result<String, DatabaseError> {
    match record.fields().get("id") {
        Some(Value::Id(value)) | Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(PlanError::Invalid("record missing valid id".to_owned()).into()),
    }
}

#[test]
fn validated_options_are_the_standard_opening_surface() {
    let root = temp_root();
    assert!(DatabaseOptions::new(&root, "../escape", schema()).is_err());

    let options = DatabaseOptions::new(&root, "chat", schema()).unwrap();
    let db = Database::open(options);
    db.init().unwrap();
    assert!(root.join("chat/sixpack.toml").is_file());
    let _ = fs::remove_dir_all(root);
}

fn schema() -> DatabaseSchema {
    let mut db = DatabaseSchema::new();
    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    db.add_table(messages).unwrap();
    db
}

fn note_schema() -> DatabaseSchema {
    let mut db = DatabaseSchema::new();
    let mut notebooks = TableSchema::new("notebooks");
    notebooks.add_field("id", PrimitiveType::Id).unwrap();
    notebooks.add_field("title", PrimitiveType::Text).unwrap();
    notebooks
        .add_field("created_at", PrimitiveType::Int)
        .unwrap();
    notebooks.add_lookup("title", false).unwrap();
    db.add_table(notebooks).unwrap();

    let mut notes = TableSchema::new("notes");
    notes.add_field("id", PrimitiveType::Id).unwrap();
    notes.add_field("notebook_id", PrimitiveType::Id).unwrap();
    notes.add_field("title", PrimitiveType::Text).unwrap();
    notes.add_field("body", PrimitiveType::Text).unwrap();
    notes.add_field("updated_at", PrimitiveType::Int).unwrap();
    notes.add_lookup("notebook_id", false).unwrap();
    notes.add_lookup("updated_at", false).unwrap();
    db.add_table(notes).unwrap();
    db
}

fn chat_schema() -> DatabaseSchema {
    let mut db = DatabaseSchema::new();
    let mut conversations = TableSchema::new("conversations");
    conversations.add_field("id", PrimitiveType::Id).unwrap();
    conversations
        .add_field("owner_id", PrimitiveType::Id)
        .unwrap();
    conversations
        .add_field("title", PrimitiveType::Text)
        .unwrap();
    conversations.add_lookup("owner_id", false).unwrap();
    db.add_table(conversations).unwrap();

    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages
        .add_field("conversation_id", PrimitiveType::Id)
        .unwrap();
    messages.add_field("role", PrimitiveType::Text).unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages
        .add_field("created_at", PrimitiveType::Int)
        .unwrap();
    messages.add_lookup("conversation_id", false).unwrap();
    db.add_table(messages).unwrap();
    db
}

#[test]
fn init_creates_empty_note_database_layout() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "notes-db", note_schema());

    db.init().unwrap();
    let db_dir = root.join("notes-db");
    assert!(db_dir.join("sixpack.toml").exists());
    assert!(db_dir.join("tables/notebooks").exists());
    assert!(db_dir.join("tables/notes").exists());
    assert!(db_dir.join("engine/notebooks.6b").exists());
    assert!(db_dir.join("engine/notes.6b").exists());

    let metadata = fs::read_to_string(db_dir.join("sixpack.toml")).unwrap();
    assert!(metadata.contains("[tables.notebooks]"));
    assert!(metadata.contains("[tables.notes]"));
    assert!(metadata.contains("next_chunk = 0"));
    assert!(metadata.contains("file = \"engine/notebooks.6b\""));
    assert!(metadata.contains("file = \"engine/notes.6b\""));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn paged_chat_histories_stay_isolated_and_survive_reopen() {
    let root = temp_root();
    let schema = chat_schema();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    db.init().unwrap();

    db.insert_many(&[
        Record::new("conversations")
            .with_id("c1")
            .unwrap()
            .with_field("owner_id", Value::Id("u1".to_owned()))
            .unwrap()
            .with_field("title", "First chat")
            .unwrap(),
        Record::new("conversations")
            .with_id("c2")
            .unwrap()
            .with_field("owner_id", Value::Id("u2".to_owned()))
            .unwrap()
            .with_field("title", "Second chat")
            .unwrap(),
    ])
    .unwrap();

    let messages = (0..150)
        .map(|index| {
            let conversation = if index % 2 == 0 { "c1" } else { "c2" };
            Record::new("messages")
                .with_id(format!("m-{index:04}"))
                .unwrap()
                .with_field("conversation_id", Value::Id(conversation.to_owned()))
                .unwrap()
                .with_field("role", if index % 4 < 2 { "user" } else { "assistant" })
                .unwrap()
                .with_field("body", format!("message {index}"))
                .unwrap()
                .with_field("created_at", index as i64)
                .unwrap()
        })
        .collect::<Vec<_>>();
    db.insert_many(&messages).unwrap();

    let mut cursor = None;
    let mut first_chat = Vec::new();
    loop {
        let page = db
            .get_page_by("messages", "conversation_id", "c1", 17, cursor.as_deref())
            .unwrap();
        first_chat.extend(page.rows);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(first_chat.len(), 75);
    assert!(
        first_chat.iter().all(|row| {
            row.fields().get("conversation_id") == Some(&Value::Id("c1".to_owned()))
        })
    );
    assert!(
        first_chat
            .windows(2)
            .all(|rows| { record_id(&rows[0]).unwrap() < record_id(&rows[1]).unwrap() })
    );

    let reopened = Database::open_local_with_schema(root.clone(), "chat", schema);
    assert_eq!(
        reopened
            .get_page_by("messages", "conversation_id", "c2", 100, None)
            .unwrap()
            .rows
            .len(),
        75
    );
    assert_eq!(reopened.count("messages").unwrap(), 150);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn chat_storage_uses_shared_table_chunks_and_one_physical_line_per_row() {
    let root = temp_root();
    let schema = chat_schema();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    db.init().unwrap();

    db.insert_many(&[
        Record::new("conversations")
            .with_id("c1")
            .unwrap()
            .with_field("owner_id", Value::Id("u1".to_owned()))
            .unwrap()
            .with_field("title", "First\tchat")
            .unwrap(),
        Record::new("conversations")
            .with_id("c2")
            .unwrap()
            .with_field("owner_id", Value::Id("u2".to_owned()))
            .unwrap()
            .with_field("title", "Second chat")
            .unwrap(),
    ])
    .unwrap();

    let rows = [
        ("m-0001", "c1", "user", "hello\tthere\nnext\\line", 1),
        ("m-0002", "c2", "user", "other conversation", 2),
        ("m-0003", "c1", "assistant", "answer one", 3),
        ("m-0004", "c2", "assistant", "answer two", 4),
    ]
    .map(|(id, conversation, role, body, created_at)| {
        Record::new("messages")
            .with_id(id)
            .unwrap()
            .with_field("conversation_id", Value::Id(conversation.to_owned()))
            .unwrap()
            .with_field("role", role)
            .unwrap()
            .with_field("body", body)
            .unwrap()
            .with_field("created_at", created_at)
            .unwrap()
    });
    db.insert_many(&rows).unwrap();

    let database_dir = root.join("chat");
    let conversation_dir = database_dir.join("tables/conversations");
    let message_dir = database_dir.join("tables/messages");
    let conversation_entries = fs::read_dir(&conversation_dir)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let message_entries = fs::read_dir(&message_dir)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(conversation_entries.len(), 1);
    assert_eq!(message_entries.len(), 1);
    assert_eq!(conversation_entries[0].file_name(), "zzz.6");
    assert_eq!(message_entries[0].file_name(), "zzz.6");
    assert!(!message_dir.join("c1").exists());
    assert!(!message_dir.join("c2").exists());
    assert!(!message_dir.join("c1.6").exists());
    assert!(!message_dir.join("c2.6").exists());

    let conversations = fs::read_to_string(conversation_entries[0].path()).unwrap();
    let conversation_rows = conversations
        .lines()
        .filter(|line| line.starts_with("R\t"))
        .collect::<Vec<_>>();
    assert_eq!(conversation_rows.len(), 2);
    assert!(conversation_rows.iter().any(|line| line.contains("\tc1\t")));
    assert!(conversation_rows.iter().any(|line| line.contains("\tc2\t")));
    assert!(conversations.contains("First\\tchat"));

    let messages = fs::read_to_string(message_entries[0].path()).unwrap();
    let message_rows = messages
        .lines()
        .filter(|line| line.starts_with("R\t"))
        .collect::<Vec<_>>();
    assert_eq!(message_rows.len(), 4);
    assert!(message_rows.iter().any(|line| line.contains("\tc1\t")));
    assert!(message_rows.iter().any(|line| line.contains("\tc2\t")));
    assert!(messages.contains("hello\\tthere\\nnext\\\\line"));
    assert!(
        message_rows
            .iter()
            .all(|line| line.split('\t').count() == 7)
    );

    assert_eq!(
        db.get_page_by("messages", "conversation_id", "c1", 100, None)
            .unwrap()
            .rows
            .len(),
        2
    );
    assert_eq!(
        db.get_page_by("messages", "conversation_id", "c2", 100, None)
            .unwrap()
            .rows
            .len(),
        2
    );
    let reopened = Database::open_local_with_schema(root.clone(), "chat", schema);
    let escaped = reopened.get_by_id("messages", "m-0001").unwrap().unwrap();
    assert_eq!(
        escaped.fields().get("body"),
        Some(&Value::Text("hello\tthere\nnext\\line".to_owned()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn message_chunks_roll_over_by_size_not_by_conversation() {
    let root = temp_root();
    let schema = chat_schema();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    db.init().unwrap();
    let large_body = "x".repeat(600_000);

    for (index, conversation) in ["c1", "c2"].into_iter().enumerate() {
        db.insert(
            &Record::new("messages")
                .with_id(format!("m-{index}"))
                .unwrap()
                .with_field("conversation_id", Value::Id(conversation.to_owned()))
                .unwrap()
                .with_field("role", "assistant")
                .unwrap()
                .with_field("body", large_body.clone())
                .unwrap()
                .with_field("created_at", index as i64)
                .unwrap(),
        )
        .unwrap();
    }

    let message_dir = root.join("chat/tables/messages");
    let mut chunk_names = fs::read_dir(&message_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    chunk_names.sort();
    assert_eq!(chunk_names, ["zzy.6", "zzz.6"]);
    assert!(!message_dir.join("c1.6").exists());
    assert!(!message_dir.join("c2.6").exists());

    let reopened = Database::open_local_with_schema(root.clone(), "chat", schema);
    assert_eq!(reopened.count("messages").unwrap(), 2);
    assert_eq!(
        reopened
            .get_page_by("messages", "conversation_id", "c1", 10, None)
            .unwrap()
            .rows
            .len(),
        1
    );
    assert_eq!(
        reopened
            .get_page_by("messages", "conversation_id", "c2", 10, None)
            .unwrap()
            .rows
            .len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn put_validates_and_appends() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());

    let first = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    let second = Record::new("messages")
        .with_id("m2")
        .unwrap()
        .with_field("body", "world")
        .unwrap();

    let one = db.put(&first).unwrap();
    let two = db.put(&second).unwrap();
    assert_eq!(one.tx_id, 1);
    assert_eq!(two.tx_id, 2);
    let db_dir = root.join("chat");
    assert!(db_dir.join("tables/messages/zzz.6").exists());
    assert!(!db_dir.join("tables/messages/zzy.6").exists());
    assert!(db_dir.join("sixpack.toml").exists());
    assert!(db_dir.join("engine/messages.6b").exists());
    let chunk = fs::read_to_string(db_dir.join("tables/messages/zzz.6")).unwrap();
    assert!(chunk.starts_with("SIX\t1\ttable\tmessages\t"));
    assert!(chunk.contains("R\t1\tm1\thello\n"));
    assert!(chunk.contains("R\t2\tm2\tworld\n"));
    assert_eq!(
        db.get(selector::id("messages", "m1"))
            .unwrap()
            .unwrap()
            .fields()
            .get("body"),
        first.fields().get("body")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn insert_fails_when_id_already_exists() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();

    db.insert(&row).unwrap();
    assert!(db.insert(&row).is_err());
    assert_eq!(db.count("messages").unwrap(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn get_and_write_accept_declarative_requests() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();

    db.write(change::add(row)).unwrap();
    let found = db.get(selector::id("messages", "m1")).unwrap().unwrap();
    assert_eq!(
        found.fields().get("body"),
        Some(&Value::Text("hello".to_owned()))
    );

    db.write(change::edit_id(
        "messages",
        "m1",
        BTreeMap::from([("body".to_owned(), Value::Text("updated".to_owned()))]),
    ))
    .unwrap();
    let updated = db.get(selector::id("messages", "m1")).unwrap().unwrap();
    assert_eq!(
        updated.fields().get("body"),
        Some(&Value::Text("updated".to_owned()))
    );

    db.write(change::remove_id("messages", "m1")).unwrap();
    assert!(db.get(selector::id("messages", "m1")).unwrap().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn insert_many_batches_rows_into_one_chunk() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let rows = vec![
        Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap(),
        Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("body", "world")
            .unwrap(),
    ];

    let results = db.insert_many(&rows).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].tx_id, 1);
    assert_eq!(results[1].tx_id, 2);
    assert!(root.join("chat/tables/messages/zzz.6").exists());
    assert!(!root.join("chat/tables/messages/zzy.6").exists());
    let chunk = fs::read_to_string(root.join("chat/tables/messages/zzz.6")).unwrap();
    assert!(chunk.contains("R\t1\tm1\thello\n"));
    assert!(chunk.contains("R\t2\tm2\tworld\n"));
    assert_eq!(db.count("messages").unwrap(), 2);
    assert_eq!(
        db.get(selector::id("messages", "m2"))
            .unwrap()
            .unwrap()
            .fields()
            .get("body"),
        Some(&Value::Text("world".to_owned()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn insert_many_rejects_duplicate_ids_before_append() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let rows = vec![
        Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap(),
        Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "world")
            .unwrap(),
    ];

    assert!(db.insert_many(&rows).is_err());
    assert!(!root.join("chat/tables/messages/zzz.6").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn insert_many_rejects_unique_lookup_duplicates_before_append() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut users = TableSchema::new("users");
    users.add_field("id", PrimitiveType::Id).unwrap();
    users.add_field("email", PrimitiveType::Text).unwrap();
    users.add_field("name", PrimitiveType::Text).unwrap();
    users.add_lookup("email", true).unwrap();
    schema.add_table(users).unwrap();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema);
    let rows = vec![
        Record::new("users")
            .with_id("u1")
            .unwrap()
            .with_field("email", "same@test.com")
            .unwrap()
            .with_field("name", "Ada")
            .unwrap(),
        Record::new("users")
            .with_id("u2")
            .unwrap()
            .with_field("email", "same@test.com")
            .unwrap()
            .with_field("name", "Ben")
            .unwrap(),
    ];

    assert!(db.insert_many(&rows).is_err());
    assert!(!root.join("chat/tables/users/zzz.6").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_many_batches_patches_into_one_chunk() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let rows = vec![
        Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap(),
        Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("body", "world")
            .unwrap(),
    ];
    db.insert_many(&rows).unwrap();

    let results = db
        .write_many(&[
            change::edit_id(
                "messages",
                "m1",
                BTreeMap::from([("body".to_owned(), Value::Text("first".to_owned()))]),
            ),
            change::edit_id(
                "messages",
                "m2",
                BTreeMap::from([("body".to_owned(), Value::Text("second".to_owned()))]),
            ),
        ])
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].tx_id, 3);
    assert_eq!(results[1].tx_id, 4);
    assert!(root.join("chat/tables/messages/zzz.6").exists());
    assert!(!root.join("chat/tables/messages/zzy.6").exists());
    let chunk = fs::read_to_string(root.join("chat/tables/messages/zzz.6")).unwrap();
    assert!(chunk.contains("R\t3\tm1\tfirst\n"));
    assert!(chunk.contains("R\t4\tm2\tsecond\n"));
    assert_eq!(
        db.get(selector::id("messages", "m2"))
            .unwrap()
            .unwrap()
            .fields()
            .get("body"),
        Some(&Value::Text("second".to_owned()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_many_batches_removes_into_one_chunk() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let rows = vec![
        Record::new("messages")
            .with_id("m1")
            .unwrap()
            .with_field("body", "hello")
            .unwrap(),
        Record::new("messages")
            .with_id("m2")
            .unwrap()
            .with_field("body", "world")
            .unwrap(),
    ];
    db.insert_many(&rows).unwrap();

    let results = db
        .write_many(&[
            change::remove_id("messages", "m1"),
            change::remove_id("messages", "m2"),
        ])
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].tx_id, 3);
    assert_eq!(results[1].tx_id, 4);
    assert_eq!(db.count("messages").unwrap(), 0);
    assert!(root.join("chat/tables/messages/zzz.6").exists());
    assert!(!root.join("chat/tables/messages/zzy.6").exists());
    let chunk = fs::read_to_string(root.join("chat/tables/messages/zzz.6")).unwrap();
    assert!(chunk.contains("D\t3\tm1\n"));
    assert!(chunk.contains("D\t4\tm2\n"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn put_replaces_existing_row() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let first = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    let second = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "updated")
        .unwrap();

    db.insert(&first).unwrap();
    db.put(&second).unwrap();
    assert_eq!(
        db.get(selector::id("messages", "m1"))
            .unwrap()
            .unwrap()
            .fields()
            .get("body"),
        second.fields().get("body")
    );
    assert_eq!(db.count("messages").unwrap(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn get_by_lookup_uses_generated_sixb_cache() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages
        .add_field("conversation_id", PrimitiveType::Id)
        .unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages.add_lookup("conversation_id", false).unwrap();
    schema.add_table(messages).unwrap();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema);

    let first = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("conversation_id", sixpack_core::Value::Id("cv1".to_owned()))
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    let second = Record::new("messages")
        .with_id("m2")
        .unwrap()
        .with_field("conversation_id", sixpack_core::Value::Id("cv1".to_owned()))
        .unwrap()
        .with_field("body", "world")
        .unwrap();

    db.put(&first).unwrap();
    db.put(&second).unwrap();
    let rows = db
        .get_many_by("messages", "conversation_id", "cv1")
        .unwrap();
    assert_eq!(rows.len(), 2);

    fs::remove_file(root.join("chat/engine/messages.6b")).unwrap();
    assert_eq!(
        db.get(selector::id("messages", "m2"))
            .unwrap()
            .unwrap()
            .fields()
            .get("body"),
        second.fields().get("body")
    );
    assert!(!root.join("chat/engine/messages.6b").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fresh_handle_uses_and_rebuilds_generated_cache() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages
        .add_field("conversation_id", PrimitiveType::Id)
        .unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages.add_lookup("conversation_id", false).unwrap();
    schema.add_table(messages).unwrap();

    let db = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("conversation_id", Value::Id("cv1".to_owned()))
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    db.insert(&row).unwrap();

    let reopened = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    let rows = reopened
        .get_many_by("messages", "conversation_id", "cv1")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].fields().get("body"),
        Some(&Value::Text("hello".to_owned()))
    );

    fs::remove_file(root.join("chat/engine/messages.6b")).unwrap();
    let cold = Database::open_local_with_schema(root.clone(), "chat", schema);
    assert_eq!(cold.count("messages").unwrap(), 1);
    assert!(root.join("chat/engine/messages.6b").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cached_and_rebuilt_reads_match_without_changing_canonical_data() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut messages = TableSchema::new("messages");
    messages.add_field("id", PrimitiveType::Id).unwrap();
    messages
        .add_field("conversation_id", PrimitiveType::Id)
        .unwrap();
    messages.add_field("body", PrimitiveType::Text).unwrap();
    messages.add_lookup("conversation_id", false).unwrap();
    schema.add_table(messages).unwrap();

    let db = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    db.init().unwrap();
    let records = (0..256)
        .map(|index| {
            Record::new("messages")
                .with_id(format!("m{index:04}"))
                .unwrap()
                .with_field("conversation_id", Value::Id(format!("cv{}", index % 8)))
                .unwrap()
                .with_field("body", format!("message {index}"))
                .unwrap()
        })
        .collect::<Vec<_>>();
    db.insert_many(&records).unwrap();
    db.rebuild_cache("messages").unwrap();
    drop(db);

    let chunk_path = root.join("chat/tables/messages/zzz.6");
    let canonical_before = fs::read(&chunk_path).unwrap();
    let cached = Database::open_local_with_schema(root.clone(), "chat", schema.clone());
    let cached_count = cached.count("messages").unwrap();
    let cached_id = cached.get(selector::id("messages", "m0128")).unwrap();
    let cached_lookup = cached
        .get_many_by("messages", "conversation_id", "cv0")
        .unwrap();
    drop(cached);

    let cache_path = root.join("chat/engine/messages.6b");
    fs::remove_file(&cache_path).unwrap();
    let rebuilt = Database::open_local_with_schema(root.clone(), "chat", schema);
    let rebuilt_count = rebuilt.count("messages").unwrap();
    let rebuilt_id = rebuilt.get(selector::id("messages", "m0128")).unwrap();
    let rebuilt_lookup = rebuilt
        .get_many_by("messages", "conversation_id", "cv0")
        .unwrap();

    assert_eq!(rebuilt_count, cached_count);
    assert_eq!(rebuilt_id, cached_id);
    assert_eq!(rebuilt_lookup, cached_lookup);
    assert!(cache_path.is_file());
    assert_eq!(fs::read(chunk_path).unwrap(), canonical_before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_removes_live_row_from_sixb_cache() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();

    db.put(&row).unwrap();
    assert!(db.get(selector::id("messages", "m1")).unwrap().is_some());
    db.delete_by_id("messages", "m1").unwrap();
    assert!(db.get(selector::id("messages", "m1")).unwrap().is_none());

    let delete_chunk = fs::read_to_string(root.join("chat/tables/messages/zzz.6")).unwrap();
    assert!(delete_chunk.contains("D\t2\tm1\n"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unique_lookup_conflicts_fail_before_append() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut users = TableSchema::new("users");
    users.add_field("id", PrimitiveType::Id).unwrap();
    users.add_field("email", PrimitiveType::Text).unwrap();
    users.add_field("name", PrimitiveType::Text).unwrap();
    users.add_lookup("email", true).unwrap();
    schema.add_table(users).unwrap();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema);

    let first = Record::new("users")
        .with_id("u1")
        .unwrap()
        .with_field("email", "same@test.com")
        .unwrap()
        .with_field("name", "Ada")
        .unwrap();
    let second = Record::new("users")
        .with_id("u2")
        .unwrap()
        .with_field("email", "same@test.com")
        .unwrap()
        .with_field("name", "Ben")
        .unwrap();

    db.insert(&first).unwrap();
    assert!(db.insert(&second).is_err());
    let first_chunk = fs::read_to_string(root.join("chat/tables/users/zzz.6")).unwrap();
    assert!(!first_chunk.contains("u2\tsame@test.com"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_many_validates_unique_lookups_against_the_final_batch_state() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut users = TableSchema::new("users");
    users.add_field("id", PrimitiveType::Id).unwrap();
    users.add_field("email", PrimitiveType::Text).unwrap();
    users.add_field("name", PrimitiveType::Text).unwrap();
    users.add_lookup("email", true).unwrap();
    schema.add_table(users).unwrap();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema);

    let first = Record::new("users")
        .with_id("u1")
        .unwrap()
        .with_field("email", "ada@test.com")
        .unwrap()
        .with_field("name", "Ada")
        .unwrap();
    let second = Record::new("users")
        .with_id("u2")
        .unwrap()
        .with_field("email", "ben@test.com")
        .unwrap()
        .with_field("name", "Ben")
        .unwrap();
    db.insert_many(&[first, second]).unwrap();

    let swapped_first = Record::new("users")
        .with_id("u1")
        .unwrap()
        .with_field("email", "ben@test.com")
        .unwrap()
        .with_field("name", "Ada")
        .unwrap();
    let swapped_second = Record::new("users")
        .with_id("u2")
        .unwrap()
        .with_field("email", "ada@test.com")
        .unwrap()
        .with_field("name", "Ben")
        .unwrap();

    db.write_many(&[change::set(swapped_first), change::set(swapped_second)])
        .unwrap();

    assert_eq!(
        db.get(selector::one("users", "email", "ben@test.com"))
            .unwrap()
            .unwrap()
            .fields()
            .get("id"),
        Some(&Value::Id("u1".to_owned()))
    );
    assert_eq!(
        db.get(selector::one("users", "email", "ada@test.com"))
            .unwrap()
            .unwrap()
            .fields()
            .get("id"),
        Some(&Value::Id("u2".to_owned()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plan_executor_patches_rows_and_preserves_fields() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();

    db.insert(&row).unwrap();
    let result = db
        .patch_by_id(
            "messages",
            "m1",
            BTreeMap::from([("body".to_owned(), Value::Text("updated".to_owned()))]),
        )
        .unwrap();

    assert_eq!(result.tx_id, 2);
    let updated = db.get(selector::id("messages", "m1")).unwrap().unwrap();
    assert_eq!(
        updated.fields().get("body"),
        Some(&Value::Text("updated".to_owned()))
    );
    assert_eq!(
        updated.fields().get("id"),
        Some(&Value::Id("m1".to_owned()))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plan_executor_removes_by_unique_lookup() {
    let root = temp_root();
    let mut schema = DatabaseSchema::new();
    let mut users = TableSchema::new("users");
    users.add_field("id", PrimitiveType::Id).unwrap();
    users.add_field("email", PrimitiveType::Text).unwrap();
    users.add_field("name", PrimitiveType::Text).unwrap();
    users.add_lookup("email", true).unwrap();
    schema.add_table(users).unwrap();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema);
    let row = Record::new("users")
        .with_id("u1")
        .unwrap()
        .with_field("email", "a@test.com")
        .unwrap()
        .with_field("name", "Ada")
        .unwrap();

    db.insert(&row).unwrap();
    let plan = PlanEnvelope::new(PlanOp::Remove, "users")
        .with_lookup("email")
        .with_key("email", Value::Text("a@test.com".to_owned()));
    db.execute_plan(plan).unwrap();

    assert!(db.get(selector::id("users", "u1")).unwrap().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plan_executor_scans_and_counts_live_rows() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    for id in ["m1", "m2", "m3"] {
        let row = Record::new("messages")
            .with_id(id)
            .unwrap()
            .with_field("body", format!("body-{id}"))
            .unwrap();
        db.insert(&row).unwrap();
    }
    db.delete_by_id("messages", "m2").unwrap();

    assert_eq!(db.count("messages").unwrap(), 2);
    let first = db.scan("messages", Some(1), None).unwrap();
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.next_cursor, Some("1".to_owned()));
    let second = db
        .scan("messages", Some(1), first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.next_cursor, None);
    let ids: Vec<_> = [first.rows, second.rows]
        .concat()
        .into_iter()
        .map(|row| record_id(&row).unwrap())
        .collect();
    assert_eq!(ids, vec!["m1".to_owned(), "m3".to_owned()]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plan_executor_rejects_bad_patch() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let row = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    db.insert(&row).unwrap();

    let err = db
        .patch_by_id(
            "messages",
            "m1",
            BTreeMap::from([("id".to_owned(), Value::Id("m2".to_owned()))]),
        )
        .unwrap_err();

    assert!(err.to_string().contains("patch cannot change id"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn put_fails_schema_mismatch() {
    let root = temp_root();
    let db = Database::open_local_with_schema(root.clone(), "chat", schema());
    let bad = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", 42i64)
        .unwrap();

    assert!(db.put(&bad).is_err());
    let _ = fs::remove_dir_all(root);
}

table!(chat_schema_example {
    id: id;
    username: text;
    score: float;
    has_premium: bool;
});

#[test]
fn macro_generates_table_schema() {
    let table = chat_schema_example::table_schema();
    let db_schema = chat_schema_example::table_database();
    assert_eq!(table.name(), "chat_schema_example");
    assert!(db_schema.table("chat_schema_example").is_some());
    assert_eq!(table.field("id").map(|f| f.kind()), Some(PrimitiveType::Id));
}

#[test]
fn macro_row_types_are_concrete() {
    let row = chat_schema_example::Row {
        id: "u1".to_string(),
        username: "mira".to_string(),
        score: 12.5,
        has_premium: false,
    };
    assert_eq!(row.id, "u1");
}

schema! {
    chat {
        id id
        owner_id id
        title text
        created_at int

        lookup owner_id
        lookup created_at
    }

    schema_users {
        id id
        email text
        name text

        lookup email unique
    }
}

#[test]
fn schema_macro_generates_database_schema() {
    let db = database_schema();
    let users = db.table("schema_users").unwrap();
    let chat = db.table("chat").unwrap();
    assert_eq!(users.field("email").unwrap().kind(), PrimitiveType::Text);
    assert!(users.lookup("email").unwrap().unique());
    assert!(!chat.lookup("owner_id").unwrap().unique());
}
