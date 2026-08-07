//! Official Topcoat + Sixpack notes example and CLI starter source.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sixpack::{Database, GetPage, Record, Value, WriteChange, schema};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Router, RouterBuilderDiscoverExt,
        content::Form,
        error::{SeeOther, see_other},
        layout, page, path_param, route,
    },
    view::{component, view},
};

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/schema.sixpack"));

const STYLES: &str = r#"
:root {
  color-scheme: light;
  --paper: #f2efe8;
  --paper-strong: #e8e2d5;
  --ink: #171713;
  --muted: #6c685e;
  --line: #cbc4b5;
  --signal: #ff4f1f;
  --signal-soft: #ffd8ca;
  --white: #fffdf8;
  font-family: Inter, ui-sans-serif, system-ui, sans-serif;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background:
    radial-gradient(circle at 12% 12%, #fff8e9 0, transparent 28rem),
    var(--paper);
  color: var(--ink);
}
button, input, textarea { font: inherit; }
button { cursor: pointer; }
.shell {
  width: min(1180px, calc(100% - 32px));
  margin: 0 auto;
  padding: 24px 0 64px;
}
.masthead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--ink);
  padding: 0 0 16px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 11px;
  color: var(--ink);
  font-size: 13px;
  font-weight: 800;
  letter-spacing: .12em;
  text-decoration: none;
  text-transform: uppercase;
}
.brand-mark {
  display: grid;
  width: 27px;
  height: 27px;
  place-items: center;
  border-radius: 50%;
  background: var(--signal);
  color: var(--white);
  font-size: 14px;
  letter-spacing: 0;
}
.status {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
}
.status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #41a663;
  box-shadow: 0 0 0 4px #d9eadc;
}
.hero {
  display: grid;
  grid-template-columns: 1.35fr .65fr;
  gap: 42px;
  padding: 64px 0 48px;
}
.kicker {
  margin: 0 0 18px;
  color: var(--signal);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  letter-spacing: .08em;
  text-transform: uppercase;
}
h1 {
  max-width: 760px;
  margin: 0;
  font-family: Georgia, Times New Roman, serif;
  font-size: clamp(58px, 9vw, 116px);
  font-weight: 500;
  letter-spacing: -.075em;
  line-height: .78;
}
h1 em { color: var(--signal); font-weight: 500; }
.hero-copy {
  align-self: end;
  border-left: 1px solid var(--line);
  padding-left: 24px;
}
.hero-copy p {
  margin: 0;
  color: var(--muted);
  font-size: 15px;
  line-height: 1.65;
}
.hero-copy strong { color: var(--ink); }
.workspace {
  display: grid;
  grid-template-columns: minmax(300px, .72fr) minmax(0, 1.28fr);
  min-height: 520px;
  border: 1px solid var(--ink);
  background: var(--white);
  box-shadow: 12px 12px 0 var(--paper-strong);
}
.composer {
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--ink);
  padding: 26px;
}
.section-label {
  margin: 0 0 24px;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  letter-spacing: .1em;
  text-transform: uppercase;
}
.composer label {
  margin-bottom: 8px;
  font-size: 12px;
  font-weight: 700;
}
.composer input,
.composer textarea {
  width: 100%;
  border: 0;
  border-bottom: 1px solid var(--line);
  border-radius: 0;
  outline: 0;
  background: transparent;
  color: var(--ink);
}
.composer input {
  margin-bottom: 26px;
  padding: 7px 0 13px;
  font-family: Georgia, Times New Roman, serif;
  font-size: 25px;
}
.composer textarea {
  min-height: 180px;
  resize: vertical;
  padding: 7px 0 14px;
  line-height: 1.6;
}
.composer input:focus,
.composer textarea:focus { border-color: var(--signal); }
.composer button {
  width: 100%;
  margin-top: auto;
  border: 1px solid var(--ink);
  border-radius: 999px;
  background: var(--ink);
  color: var(--white);
  padding: 13px 18px;
  font-weight: 750;
}
.composer button:hover {
  border-color: var(--signal);
  background: var(--signal);
}
.notes {
  min-width: 0;
  padding: 26px;
}
.notes-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 18px;
}
.notes-head .section-label { margin: 0; }
.count {
  color: var(--signal);
  font-family: Georgia, Times New Roman, serif;
  font-size: 24px;
  font-style: italic;
}
.note-list {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  border-top: 1px solid var(--line);
}
.note {
  min-height: 190px;
  border-bottom: 1px solid var(--line);
  padding: 22px 20px 20px 0;
}
.note:nth-child(even) {
  border-left: 1px solid var(--line);
  padding-left: 20px;
}
.note h2 {
  margin: 0 0 12px;
  font-family: Georgia, Times New Roman, serif;
  font-size: 25px;
  font-weight: 500;
  letter-spacing: -.025em;
}
.note-body {
  min-height: 68px;
  margin: 0;
  color: var(--muted);
  font-size: 14px;
  line-height: 1.55;
  white-space: pre-wrap;
}
.note-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 18px;
}
.note-meta span {
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
}
.delete {
  border: 0;
  background: transparent;
  color: var(--muted);
  padding: 0;
  font-size: 11px;
  text-decoration: underline;
  text-underline-offset: 3px;
}
.delete:hover { color: var(--signal); }
.empty {
  display: grid;
  min-height: 320px;
  place-items: center;
  border-top: 1px solid var(--line);
  color: var(--muted);
  text-align: center;
}
.footnote {
  display: flex;
  justify-content: space-between;
  gap: 24px;
  margin-top: 28px;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
}
@media (max-width: 800px) {
  .hero, .workspace { grid-template-columns: 1fr; }
  .hero { gap: 28px; padding-top: 46px; }
  .hero-copy { border-left: 0; padding-left: 0; }
  .composer { min-height: 480px; border-right: 0; border-bottom: 1px solid var(--ink); }
}
@media (max-width: 540px) {
  .shell { width: min(100% - 20px, 1180px); }
  .note-list { grid-template-columns: 1fr; }
  .note:nth-child(even) { border-left: 0; padding-left: 0; }
  .footnote { flex-direction: column; }
}
"#;

#[tokio::main]
async fn main() {
    let database = database_path();
    let state = AppState::open(database).expect("could not open the sixpack notes database");
    let router = Router::builder().discover().app_context(state).build();

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .expect("could not bind the Topcoat server");
    let address = listener
        .local_addr()
        .expect("could not read the Topcoat server address");
    let browser_host = if address.ip().is_unspecified() {
        "127.0.0.1".to_owned()
    } else {
        address.ip().to_string()
    };
    println!(
        "topcoat sixpack http://{}:{}/",
        browser_host,
        address.port()
    );

    topcoat::serve(listener, router)
        .await
        .expect("Topcoat server failed");
}

fn database_path() -> PathBuf {
    std::env::var_os("SIXPACK_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("could not read the current directory")
                .join("data")
        })
}

#[derive(Debug)]
struct AppState {
    db: Database,
    next_id: AtomicU64,
}

impl AppState {
    fn open(database: PathBuf) -> Result<Self> {
        let db = Database::open_path_with_schema(database, database_schema())?;
        db.init()?;
        let state = Self {
            db,
            next_id: AtomicU64::new(0),
        };
        state.seed_if_empty()?;
        state.refresh_projection();
        Ok(state)
    }

    fn notes(&self) -> Result<Vec<Note>> {
        let mut notes = self
            .db
            .get(GetPage::new("notes").limit(1_000))?
            .rows
            .into_iter()
            .map(Note::try_from)
            .collect::<Result<Vec<_>>>()?;
        notes.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(notes)
    }

    fn create_note(&self, title: &str, body: &str) -> Result<()> {
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = format!("note-{}-{sequence}", now_millis());
        self.db.write(WriteChange::add_record(note_record(
            &id,
            title,
            body,
            now_seconds(),
        )?))?;
        self.refresh_projection();
        Ok(())
    }

    fn delete_note(&self, id: &str) -> Result<()> {
        self.db
            .write(WriteChange::remove("notes", "id", Value::Id(id.to_owned())))?;
        self.refresh_projection();
        Ok(())
    }

    fn seed_if_empty(&self) -> Result<()> {
        if !self.notes()?.is_empty() {
            return Ok(());
        }
        for (id, title, body, updated_at) in [
            (
                "note-topcoat",
                "Rendered in Rust",
                "This card is server-rendered by Topcoat's view! macro.",
                now_seconds(),
            ),
            (
                "note-sixpack",
                "Stored close to home",
                "Create a note and Sixpack appends it to a local .6 table.",
                now_seconds().saturating_sub(1),
            ),
        ] {
            self.db.write(WriteChange::set_record(note_record(
                id, title, body, updated_at,
            )?))?;
        }
        Ok(())
    }

    fn refresh_projection(&self) {
        if let Err(error) = self.db.write_projection() {
            eprintln!("projection refresh failed after a committed write: {error}");
        }
    }
}

fn state(cx: &Cx) -> &AppState {
    app_context(cx)
}

#[layout("/")]
async fn document(slot: Result) -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta
                    name="description"
                    content="A local-first notes page rendered by Topcoat and stored by Sixpack."
                >
                <title>"Topcoat Sixpack"</title>
                <style>(STYLES)</style>
                topcoat::dev::script()
            </head>
            <body>(slot?)</body>
        </html>
    }
}

#[page("/")]
async fn home(cx: &Cx) -> Result {
    let notes = state(cx).notes()?;
    let note_count = notes.len();

    view! {
        <main class="shell">
            <header class="masthead">
                <a class="brand" href="/">
                    <span class="brand-mark">"6"</span>
                    "Topcoat × Sixpack"
                </a>
                <div class="status">
                    <span class="status-dot"></span>
                    "local / ready"
                </div>
            </header>

            <section class="hero">
                <div>
                    <p class="kicker">"Rust all the way through"</p>
                    <h1>
                        "Topcoat "
                        <em>"Sixpack"</em>
                    </h1>
                </div>
                <div class="hero-copy">
                    <p>
                        <strong>"One singular page."</strong>
                        " Topcoat renders the interface on the server. Sixpack keeps every note in inspectable local files."
                    </p>
                </div>
            </section>

            <section class="workspace">
                <form class="composer" method="post" action="/notes">
                    <p class="section-label">"01 / Compose"</p>
                    <label for="title">"Title"</label>
                    <input
                        id="title"
                        name="title"
                        type="text"
                        maxlength="100"
                        placeholder="A sharp thought"
                        required=""
                    >
                    <label for="body">"Note"</label>
                    <textarea
                        id="body"
                        name="body"
                        maxlength="2000"
                        placeholder="Write it down before it disappears."
                        required=""
                    ></textarea>
                    <button type="submit">"Commit to Sixpack"</button>
                </form>

                <div class="notes">
                    <div class="notes-head">
                        <p class="section-label">"02 / Local notes"</p>
                        <span class="count">(note_count)</span>
                    </div>
                    if notes.is_empty() {
                        <div class="empty">
                            <p>"Your first note has somewhere to land."</p>
                        </div>
                    } else {
                        <div class="note-list">
                            for note in &notes {
                                note_card(note: note)
                            }
                        </div>
                    }
                </div>
            </section>

            <footer class="footnote">
                <span>"UI: topcoat::view! + discovered routes"</span>
                <span>"DATA: tables/notes/*.6 + engine/notes.6b"</span>
            </footer>
        </main>
    }
}

#[component]
async fn note_card(note: &Note) -> Result {
    view! {
        <article class="note">
            <h2>(&note.title)</h2>
            <p class="note-body">(&note.body)</p>
            <div class="note-meta">
                <span>(format!("updated {}", note.updated_at))</span>
                <form method="post" action=(("/notes/", &note.id, "/delete"))>
                    <button
                        class="delete"
                        type="submit"
                        aria-label=(format!("Delete {}", note.title))
                    >
                        "delete"
                    </button>
                </form>
            </div>
        </article>
    }
}

#[derive(Debug, Deserialize)]
struct NewNote {
    title: String,
    body: String,
}

#[route(POST "/notes")]
async fn create_note(cx: &Cx, Form(input): Form<NewNote>) -> Result<SeeOther> {
    let title = input.title.trim();
    let body = input.body.trim();
    if !title.is_empty() && !body.is_empty() {
        state(cx).create_note(title, body)?;
    }
    Ok(see_other("/"))
}

#[path_param]
struct NoteId(str);

#[route(POST "/notes/{note_id}/delete")]
async fn delete_note(cx: &Cx) -> Result<SeeOther> {
    state(cx).delete_note(path_param::<NoteId>(cx))?;
    Ok(see_other("/"))
}

#[derive(Debug)]
struct Note {
    id: String,
    title: String,
    body: String,
    updated_at: i64,
}

impl TryFrom<Record> for Note {
    type Error = topcoat::Error;

    fn try_from(record: Record) -> Result<Self> {
        Ok(Self {
            id: required_id(&record, "id")?,
            title: required_text(&record, "title")?,
            body: required_text(&record, "body")?,
            updated_at: required_int(&record, "updated_at")?,
        })
    }
}

fn note_record(id: &str, title: &str, body: &str, updated_at: i64) -> Result<Record> {
    Ok(Record::new("notes")
        .with_id(id)?
        .with_field("title", title)?
        .with_field("body", body)?
        .with_field("updated_at", updated_at)?)
}

fn required_id(record: &Record, field: &str) -> Result<String> {
    match record.fields().get(field) {
        Some(Value::Id(value)) => Ok(value.clone()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("missing id field `{field}`"),
        )
        .into()),
    }
}

fn required_text(record: &Record, field: &str) -> Result<String> {
    match record.fields().get(field) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("missing text field `{field}`"),
        )
        .into()),
    }
}

fn required_int(record: &Record, field: &str) -> Result<i64> {
    match record.fields().get(field) {
        Some(Value::Int(value)) => Ok(*value),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("missing int field `{field}`"),
        )
        .into()),
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_secs() as i64
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_round_trip_through_sixpack() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::open(root.path().join("topcoat-notes")).unwrap();

        state
            .create_note("Test note", "Written through the Topcoat example")
            .unwrap();
        let created = state
            .notes()
            .unwrap()
            .into_iter()
            .find(|note| note.title == "Test note")
            .unwrap();
        assert_eq!(created.body, "Written through the Topcoat example");
        assert!(root.path().join("topcoat-notes/projection.html").is_file());

        state.delete_note(&created.id).unwrap();
        assert!(
            state
                .notes()
                .unwrap()
                .into_iter()
                .all(|note| note.id != created.id)
        );
    }

    #[test]
    fn projection_failure_does_not_misreport_a_committed_note() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("topcoat-notes");
        let state = AppState::open(database.clone()).unwrap();
        let projection = database.join("projection.html");
        std::fs::remove_file(&projection).unwrap();
        std::fs::create_dir(&projection).unwrap();

        state
            .create_note("Still committed", "The HTML projection is derived state.")
            .unwrap();

        assert!(
            state
                .notes()
                .unwrap()
                .into_iter()
                .any(|note| note.title == "Still committed")
        );
    }
}
