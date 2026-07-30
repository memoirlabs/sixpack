use std::path::{Path, PathBuf};

use sixpack::{Database, DatabaseError};

include!(concat!(env!("OUT_DIR"), "/ai_chat_notes_schema.rs"));

use sixpack_generated_schema as sdk;

fn open_database(path: &Path) -> Result<Database, DatabaseError> {
    Database::open_path_with_schema(path, sdk::database_schema())
}

fn run_demo(database: &Path) -> Result<(), DatabaseError> {
    let db = open_database(database)?;
    db.init()?;
    refresh_projection(&db);

    db.write(sdk::conversations::set(sdk::conversations::Row {
        id: "conversation-0001".to_owned(),
        owner_id: "user-0001".to_owned(),
        title: "Release planning".to_owned(),
        created_at: 1,
        updated_at: 1,
        archived: false,
    }))?;
    refresh_projection(&db);

    db.write(sdk::messages::set(sdk::messages::Row {
        id: "message-0001".to_owned(),
        conversation_id: "conversation-0001".to_owned(),
        role: "user".to_owned(),
        body: "Take notes while we prepare the release.".to_owned(),
        status: "completed".to_owned(),
        model: String::new(),
        created_at: 2,
        sequence: 1,
    }))?;
    refresh_projection(&db);

    db.write(sdk::messages::set(sdk::messages::Row {
        id: "message-0002".to_owned(),
        conversation_id: "conversation-0001".to_owned(),
        role: "assistant".to_owned(),
        body: String::new(),
        status: "streaming".to_owned(),
        model: "example-model".to_owned(),
        created_at: 3,
        sequence: 2,
    }))?;
    refresh_projection(&db);
    db.write(sdk::messages::edit(
        sdk::messages::key::id("message-0002"),
        sdk::messages::Patch::new()
            .body("I captured the release checklist.")
            .status("completed"),
    ))?;
    refresh_projection(&db);

    db.write(sdk::notes::set(sdk::notes::Row {
        id: "note-0001".to_owned(),
        owner_id: "user-0001".to_owned(),
        title: "Release checklist".to_owned(),
        body: "Verify chat persistence, corruption recovery, and documentation.".to_owned(),
        source_kind: "conversation".to_owned(),
        source_id: "conversation-0001".to_owned(),
        created_at: 4,
        updated_at: 4,
    }))?;
    refresh_projection(&db);

    let messages = db.get(sdk::messages::by::conversation_id("conversation-0001"))?;
    let notes = db.get(sdk::notes::by::owner_id("user-0001"))?;
    println!(
        "stored {} message(s) and {} note(s) under {}",
        messages.len(),
        notes.len(),
        database.display()
    );
    println!("projection {}", database.join("projection.html").display());
    Ok(())
}

fn refresh_projection(db: &Database) {
    if let Err(error) = db.write_projection() {
        eprintln!("projection refresh failed: {error}");
    }
}

fn temporary_database_path() -> PathBuf {
    std::env::temp_dir()
        .join(format!("sixpack-ai-chat-notes-{}", std::process::id()))
        .join("assistant")
}

fn database_path() -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut database = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--database" => {
                if database.is_some() {
                    return Err("database path specified more than once".into());
                }
                database = Some(PathBuf::from(
                    args.next().ok_or("--database requires a path")?,
                ));
            }
            "--out" => {
                if database.is_some() {
                    return Err("database path specified more than once".into());
                }
                let root = PathBuf::from(args.next().ok_or("--out requires a path")?);
                database = Some(root.join("assistant"));
            }
            "-h" | "--help" => {
                println!("ai-chat-notes [--database <path>]");
                println!();
                println!("The database defaults to a new temporary directory.");
                return Ok(None);
            }
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    Ok(Some(database.unwrap_or_else(temporary_database_path)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(database) = database_path()? else {
        return Ok(());
    };
    run_demo(&database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation(id: &str, owner_id: &str) -> sdk::conversations::Row {
        sdk::conversations::Row {
            id: id.to_owned(),
            owner_id: owner_id.to_owned(),
            title: format!("Chat {id}"),
            created_at: 1,
            updated_at: 1,
            archived: false,
        }
    }

    fn message(
        id: &str,
        conversation_id: &str,
        role: &str,
        body: &str,
        status: &str,
        sequence: i64,
    ) -> sdk::messages::Row {
        sdk::messages::Row {
            id: id.to_owned(),
            conversation_id: conversation_id.to_owned(),
            role: role.to_owned(),
            body: body.to_owned(),
            status: status.to_owned(),
            model: if role == "assistant" {
                "test-model".to_owned()
            } else {
                String::new()
            },
            created_at: sequence,
            sequence,
        }
    }

    fn note(id: &str, owner_id: &str, source_id: &str) -> sdk::notes::Row {
        sdk::notes::Row {
            id: id.to_owned(),
            owner_id: owner_id.to_owned(),
            title: "Initial title".to_owned(),
            body: "Initial body".to_owned(),
            source_kind: "conversation".to_owned(),
            source_id: source_id.to_owned(),
            created_at: 10,
            updated_at: 10,
        }
    }

    #[test]
    fn demo_reconnects_to_the_same_database_without_duplicate_rows() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("custom-assistant");

        run_demo(&database).unwrap();
        run_demo(&database).unwrap();

        let db = open_database(&database).unwrap();
        assert_eq!(db.get(sdk::conversations::count()).unwrap(), 1);
        assert_eq!(db.get(sdk::messages::count()).unwrap(), 2);
        assert_eq!(db.get(sdk::notes::count()).unwrap(), 1);
        assert!(database.join("projection.html").is_file());
    }

    #[test]
    fn typed_chat_and_notes_flow_survives_reopen_and_paginates() {
        let root = tempfile::tempdir().unwrap();
        let db = open_database(&root.path().join("assistant")).unwrap();
        db.init().unwrap();
        db.write(sdk::conversations::add(conversation("c-0001", "u-0001")))
            .unwrap();

        db.write(sdk::messages::add(message(
            "m-0001",
            "c-0001",
            "user",
            "Remember that tabs\tand newlines\nstay intact 🤖",
            "completed",
            1,
        )))
        .unwrap();
        db.write(sdk::messages::add(message(
            "m-0002",
            "c-0001",
            "assistant",
            "",
            "streaming",
            2,
        )))
        .unwrap();
        db.write(sdk::messages::edit(
            sdk::messages::key::id("m-0002"),
            sdk::messages::Patch::new()
                .body("Final durable assistant response")
                .status("completed"),
        ))
        .unwrap();
        db.write(sdk::messages::add(message(
            "m-0003",
            "c-0001",
            "tool",
            "tool output",
            "completed",
            3,
        )))
        .unwrap();

        let (first, cursor) = db
            .get(sdk::messages::by::conversation_id("c-0001").page(2))
            .unwrap();
        assert_eq!(
            first.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            ["m-0001", "m-0002"]
        );
        let (second, next) = db
            .get(
                sdk::messages::by::conversation_id("c-0001")
                    .page(2)
                    .cursor(cursor.unwrap()),
            )
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, "m-0003");
        assert_eq!(next, None);

        db.write(sdk::notes::add(note("n-0001", "u-0001", "c-0001")))
            .unwrap();
        db.write(sdk::notes::edit(
            sdk::notes::key::id("n-0001"),
            sdk::notes::Patch::new()
                .title("Release facts")
                .body("Chat and note state survived a cold reopen.")
                .updated_at(11),
        ))
        .unwrap();

        drop(db);
        let cold = open_database(&root.path().join("assistant")).unwrap();
        let assistant = cold.get(sdk::messages::by::id("m-0002")).unwrap().unwrap();
        assert_eq!(assistant.status, "completed");
        assert_eq!(assistant.body, "Final durable assistant response");
        let user = cold.get(sdk::messages::by::id("m-0001")).unwrap().unwrap();
        assert_eq!(
            user.body,
            "Remember that tabs\tand newlines\nstay intact 🤖"
        );
        let notes = cold.get(sdk::notes::by::source_id("c-0001")).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Release facts");
    }

    #[test]
    fn independent_handles_share_chat_and_note_commits() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("assistant");
        let first = open_database(&database).unwrap();
        let second = open_database(&database).unwrap();
        first.init().unwrap();

        first
            .write(sdk::conversations::add(conversation("c-0001", "u-0001")))
            .unwrap();
        assert!(
            second
                .get(sdk::conversations::by::id("c-0001"))
                .unwrap()
                .is_some()
        );

        second
            .write(sdk::messages::add(message(
                "m-0001",
                "c-0001",
                "user",
                "hello from access point two",
                "completed",
                1,
            )))
            .unwrap();
        assert_eq!(
            first
                .get(sdk::messages::by::conversation_id("c-0001"))
                .unwrap()
                .len(),
            1
        );

        first
            .write(sdk::notes::add(note("n-0001", "u-0001", "c-0001")))
            .unwrap();
        second
            .write(sdk::notes::edit(
                sdk::notes::key::id("n-0001"),
                sdk::notes::Patch::new().body("updated from access point two"),
            ))
            .unwrap();
        assert_eq!(
            first
                .get(sdk::notes::by::id("n-0001"))
                .unwrap()
                .unwrap()
                .body,
            "updated from access point two"
        );
    }

    #[test]
    fn duplicate_message_retry_and_invalid_batch_do_not_duplicate_chat_rows() {
        let root = tempfile::tempdir().unwrap();
        let db = open_database(&root.path().join("assistant")).unwrap();
        db.init().unwrap();
        db.write(sdk::conversations::add(conversation("c-0001", "u-0001")))
            .unwrap();
        db.write(sdk::messages::add(message(
            "m-0001",
            "c-0001",
            "user",
            "retry-safe message",
            "completed",
            1,
        )))
        .unwrap();

        assert!(
            db.write(sdk::messages::add(message(
                "m-0001",
                "c-0001",
                "user",
                "retry-safe message",
                "completed",
                1,
            )))
            .is_err()
        );
        assert_eq!(db.get(sdk::messages::count()).unwrap(), 1);

        let changes = [
            sdk::messages::add(message(
                "m-0002",
                "c-0001",
                "assistant",
                "first batch row",
                "completed",
                2,
            )),
            sdk::messages::add(message(
                "m-0002",
                "c-0001",
                "assistant",
                "duplicate batch id",
                "completed",
                3,
            )),
        ];
        assert!(db.write_many(&changes).is_err());
        assert_eq!(db.get(sdk::messages::count()).unwrap(), 1);
    }
}
