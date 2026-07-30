use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sixpack::{Database, GetPage, Record, Value, WriteChange, schema};

include!("../schema.sixpack");

const HTML: &str = include_str!("../static/index.html");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args()?;
    if config.reset && config.database.exists() {
        fs::remove_dir_all(&config.database)?;
    }

    let app = Arc::new(Mutex::new(App::open(config.database)?));
    let listener = TcpListener::bind(format!("{}:{}", config.host, config.port))?;
    println!("sixpack ai demo http://{}/", listener.local_addr()?);
    println!(
        "database {}",
        app.lock()
            .map_err(|_| "app lock poisoned")?
            .db
            .path()
            .display()
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, Arc::clone(&app)) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

struct Config {
    host: String,
    port: u16,
    database: PathBuf,
    reset: bool,
}

impl Config {
    fn from_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut host = "127.0.0.1".to_owned();
        let mut port = 4766;
        let mut database = None;
        let mut reset = false;
        let mut args = std::env::args().skip(1);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--host" => host = args.next().ok_or("--host requires a value")?,
                "--port" => port = args.next().ok_or("--port requires a value")?.parse()?,
                "--database" => {
                    database = Some(PathBuf::from(
                        args.next().ok_or("--database requires a value")?,
                    ));
                }
                "--reset" => reset = true,
                "-h" | "--help" => {
                    println!(
                        "sixpack-ai-ping [--database <path>] [--host <host>] [--port <port>] [--reset]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument `{other}`").into()),
            }
        }
        Ok(Self {
            host,
            port,
            database: database.unwrap_or_else(|| {
                std::env::temp_dir().join(format!("sixpack-ai-ping-{}", now_millis()))
            }),
            reset,
        })
    }
}

struct App {
    db: Database,
    revision: u64,
    last_write_ms: Option<f64>,
}

impl App {
    fn open(database: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Database::open_path_with_schema(database, database_schema())?;
        db.init()?;
        db.write_projection()?;
        Ok(Self {
            db,
            revision: 0,
            last_write_ms: None,
        })
    }

    fn ping(&mut self, input: PingInput) -> Result<PingResult, Box<dyn std::error::Error>> {
        let message = input.message.trim();
        if message.is_empty() {
            return Err("message cannot be empty".into());
        }
        let reply = "ping".to_owned();
        let sequence = self.messages()?.len() as i64;
        let stamp = now_millis();
        let user = message_record(
            &format!("message-{stamp}-user"),
            "user",
            message,
            sequence + 1,
        )?;
        let assistant = message_record(
            &format!("message-{stamp}-assistant"),
            "assistant",
            &reply,
            sequence + 2,
        )?;
        let write_started = Instant::now();
        self.db.write_many(&[
            WriteChange::add_record(user),
            WriteChange::add_record(assistant),
        ])?;
        let write_ms = ms(write_started.elapsed());
        self.last_write_ms = Some(write_ms);
        self.revision = self.revision.saturating_add(1);
        let projection_started = Instant::now();
        self.db.write_projection()?;
        Ok(PingResult {
            reply,
            timings: Timings {
                read_ms: None,
                write_ms: Some(write_ms),
                projection_ms: Some(ms(projection_started.elapsed())),
            },
        })
    }

    fn state(&self) -> Result<StateDto, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let messages = self.messages()?;
        Ok(StateDto {
            revision: self.revision,
            messages,
            timings: Timings {
                read_ms: Some(ms(started.elapsed())),
                write_ms: self.last_write_ms,
                projection_ms: None,
            },
        })
    }

    fn messages(&self) -> Result<Vec<MessageDto>, Box<dyn std::error::Error>> {
        let mut messages = self
            .db
            .get(GetPage::new("messages").limit(1_000))?
            .rows
            .into_iter()
            .map(MessageDto::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        messages.sort_by_key(|message| message.sequence);
        Ok(messages)
    }
}

fn message_record(
    id: &str,
    role: &str,
    body: &str,
    sequence: i64,
) -> Result<Record, Box<dyn std::error::Error>> {
    Ok(Record::new("messages")
        .with_id(id)?
        .with_field("role", role)?
        .with_field("body", body)?
        .with_field("sequence", sequence)?
        .with_field("created_at", now_seconds())?)
}

#[derive(Deserialize)]
struct PingInput {
    message: String,
}

#[derive(Serialize)]
struct PingResult {
    reply: String,
    timings: Timings,
}

#[derive(Serialize)]
struct StateDto {
    revision: u64,
    messages: Vec<MessageDto>,
    timings: Timings,
}

#[derive(Serialize)]
struct MessageDto {
    id: String,
    role: String,
    body: String,
    sequence: i64,
    created_at: i64,
}

impl TryFrom<Record> for MessageDto {
    type Error = Box<dyn std::error::Error>;

    fn try_from(record: Record) -> Result<Self, Self::Error> {
        Ok(Self {
            id: required_id(&record, "id")?,
            role: required_text(&record, "role")?,
            body: required_text(&record, "body")?,
            sequence: required_int(&record, "sequence")?,
            created_at: required_int(&record, "created_at")?,
        })
    }
}

#[derive(Serialize)]
struct Timings {
    #[serde(skip_serializing_if = "Option::is_none")]
    read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    write_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_ms: Option<f64>,
}

fn required_id(record: &Record, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Id(value)) => Ok(value.clone()),
        _ => Err(format!("missing id field `{field}`").into()),
    }
}

fn required_text(record: &Record, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(format!("missing text field `{field}`").into()),
    }
}

fn required_int(record: &Record, field: &str) -> Result<i64, Box<dyn std::error::Error>> {
    match record.fields().get(field) {
        Some(Value::Int(value)) => Ok(*value),
        _ => Err(format!("missing int field `{field}`").into()),
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
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

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn handle_connection(
    stream: TcpStream,
    app: Arc<Mutex<App>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = read_request(&stream)?;
    let response = route(request, app);
    write_response(stream, response)
}

fn read_request(stream: &TcpStream) -> Result<Request, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("missing method")?.to_owned();
    let raw_path = parts.next().ok_or("missing path")?;
    let path = raw_path.split('?').next().unwrap_or(raw_path).to_owned();
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
        {
            content_len = value.trim().parse()?;
        }
    }
    let mut body = vec![0u8; content_len];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, body })
}

fn route(request: Request, app: Arc<Mutex<App>>) -> Response {
    match route_inner(request, app) {
        Ok(response) => response,
        Err(error) => Response::json(
            500,
            &serde_json::json!({
                "error": error.to_string()
            }),
        ),
    }
}

fn route_inner(
    request: Request,
    app: Arc<Mutex<App>>,
) -> Result<Response, Box<dyn std::error::Error>> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Ok(Response::html(HTML)),
        ("GET", "/api/state") => {
            let app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.state()?))
        }
        ("POST", "/api/ai/ping") => {
            let input = serde_json::from_slice::<PingInput>(&request.body)?;
            let mut app = app.lock().map_err(|_| "app lock poisoned")?;
            Ok(Response::json(200, &app.ping(input)?))
        }
        _ => Ok(Response::json(
            404,
            &serde_json::json!({ "error": "not found" }),
        )),
    }
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn html(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }

    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec(value).expect("json serialization failed"),
        }
    }
}

fn write_response(
    mut stream: TcpStream,
    response: Response,
) -> Result<(), Box<dyn std::error::Error>> {
    let status_text = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_writes_both_sides_of_the_exchange() {
        let path = std::env::temp_dir().join(format!("sixpack-ai-test-{}", now_millis()));
        let mut app = App::open(path.clone()).unwrap();
        let result = app
            .ping(PingInput {
                message: "ping".to_owned(),
            })
            .unwrap();
        let messages = app.messages().unwrap();

        assert_eq!(result.reply, "ping");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].body, "ping");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].body, "ping");
        assert!(path.join("projection.html").is_file());
        let _ = fs::remove_dir_all(path);
    }
}
