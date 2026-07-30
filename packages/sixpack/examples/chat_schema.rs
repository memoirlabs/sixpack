//! Small schema-first example for a basic AI chat app.
//!
//! The example stays intentionally small and only uses the primitive types:
//! `id`, `text`, `int`, `float`, `bool`.

use sixpack::{Database, Record, Value, change};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod chat_schema {
    use sixpack::schema;

    include!("chat_schema.sixpack");
}

fn temporary_database_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    path.push(format!("sixpack-chat-example-{stamp}"));
    path
}

fn database_path() -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut database = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
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
                database = Some(root.join("chat"));
            }
            "-h" | "--help" => {
                println!("chat_schema [--database <path>]");
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
    let db = Database::open_path_with_schema(&database, chat_schema::database_schema())?;
    db.init()?;

    let user = Record::new("users")
        .with_id("u1")?
        .with_field("name", "Mira")?
        .with_field("email", "mira@example.com")?
        .with_field("is_ai_user", false)?;

    let convo = Record::new("conversations")
        .with_id("c1")?
        .with_field("owner_id", Value::Id("u1".to_owned()))?
        .with_field("title", "Demo chat")?
        .with_field("created_at", 1_707_000_000_i64)?;

    let message = Record::new("messages")
        .with_id("m1")?
        .with_field("conversation_id", Value::Id("c1".to_owned()))?
        .with_field("sender_id", Value::Id("u1".to_owned()))?
        .with_field("body", "hello from the sixpack example")?
        .with_field("created_at", 1_707_000_001_i64)?;

    db.write(change::set(user))?;
    db.write(change::set(convo))?;
    db.write(change::set(message))?;

    let projection = db.write_projection()?;
    println!("database {}", database.display());
    println!("projection {}", projection.path.display());
    Ok(())
}
