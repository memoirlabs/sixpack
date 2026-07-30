use std::fs;
use std::io::{BufRead, BufReader, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::CliError;

const NOTES_MAIN: &str = include_str!("../templates/notes/src/main.rs");
const NOTES_HTML: &str = include_str!("../templates/notes/static/index.html");
const NOTES_SCHEMA: &str = include_str!("../templates/notes/schema.sixpack");
const AI_MAIN: &str = include_str!("../templates/ai/src/main.rs");
const AI_HTML: &str = include_str!("../templates/ai/static/index.html");
const AI_SCHEMA: &str = include_str!("../templates/ai/schema.sixpack");
const TOPCOAT_MAIN: &str = include_str!("../templates/topcoat/src/main.rs");
const TOPCOAT_SCHEMA: &str = include_str!("../templates/topcoat/schema.sixpack");
const TOPCOAT_CONFIG: &str = include_str!("../templates/topcoat/Topcoat.toml");
const MINIMAL_SCHEMA: &str = r#"schema! {
  notes {
    id id
    title text
    body text
    updated_at int

    lookup updated_at
  }
}
"#;
const MINIMAL_MAIN: &str = r#"use sixpack::{Database, schema};

include!("../schema.sixpack");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = std::env::current_dir()?.join("data");
    let db = Database::open_path_with_schema(&database, database_schema())?;
    db.init()?;
    let projection = db.write_projection()?;

    println!("database {}", database.display());
    println!("projection {}", projection.path.display());
    Ok(())
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Template {
    Notes,
    Ai,
    Topcoat,
    Minimal,
}

impl Template {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "notes" => Ok(Self::Notes),
            "ai" | "ai-ping" => Ok(Self::Ai),
            "topcoat" | "topcoat-notes" => Ok(Self::Topcoat),
            "minimal" => Ok(Self::Minimal),
            other => Err(CliError::Usage(format!(
                "unknown template `{other}`; expected `notes`, `ai`, `topcoat`, or `minimal`"
            ))),
        }
    }
}

pub(crate) fn run(
    current_dir: &Path,
    mut args: impl Iterator<Item = String>,
) -> Result<(), CliError> {
    let mut destination = None;
    let mut template = None;
    let mut start = true;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                reject_remaining(&mut args)?;
                print_help();
                return Ok(());
            }
            "--template" => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--template requires a value".to_owned()))?;
                template = Some(Template::parse(&value)?);
            }
            "--no-start" => start = false,
            value if value.starts_with('-') => {
                return Err(CliError::UnexpectedArgument(value.to_owned()));
            }
            value if destination.is_none() => destination = Some(PathBuf::from(value)),
            value => return Err(CliError::UnexpectedArgument(value.to_owned())),
        }
    }

    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    print_logo(interactive);

    let destination = match destination {
        Some(path) => path,
        None if interactive => {
            let value: String = cliclack::input("Where should we create the project?")
                .default_input("./sixpack-demo")
                .interact()
                .map_err(prompt_error)?;
            PathBuf::from(value)
        }
        None => PathBuf::from("sixpack-demo"),
    };
    let template = match template {
        Some(template) => template,
        None if interactive => cliclack::select("What do you want to start with?")
            .initial_value(Template::Notes)
            .item(
                Template::Notes,
                "Notes",
                "Rust CRUD app + live database state",
            )
            .item(
                Template::Ai,
                "Chat app",
                "local echo endpoint + live message database",
            )
            .item(
                Template::Topcoat,
                "Topcoat Notes",
                "Topcoat full-stack page + local sixpack notes",
            )
            .item(
                Template::Minimal,
                "Minimal",
                "schema + Rust binary + generated projection",
            )
            .interact()
            .map_err(prompt_error)?,
        None => {
            return Err(CliError::Usage(
                "choose a template with `--template notes`, `--template ai`, `--template topcoat`, or `--template minimal`".to_owned(),
            ));
        }
    };

    let destination = if destination.is_absolute() {
        destination
    } else {
        current_dir.join(destination)
    };
    let package_name = package_name(&destination)?;

    let create_spinner = interactive.then(cliclack::spinner);
    if let Some(spinner) = &create_spinner {
        spinner.start("Putting the little pieces together");
    }
    scaffold(&destination, &package_name, template)?;
    if let Some(spinner) = &create_spinner {
        spinner.stop("Project created");
    }
    let build_spinner = interactive.then(cliclack::spinner);
    if let Some(spinner) = &build_spinner {
        spinner.start("Building the Rust binary");
    }
    build(&destination)?;
    if let Some(spinner) = &build_spinner {
        spinner.stop("Rust binary ready");
    }

    println!("project {}", destination.display());
    if !start {
        let run = if template == Template::Topcoat {
            "topcoat dev"
        } else {
            "cargo run"
        };
        println!("next cd {} && {run}", destination.display());
        return Ok(());
    }

    match template {
        Template::Notes | Template::Ai | Template::Topcoat => {
            let start_spinner = interactive.then(cliclack::spinner);
            if let Some(spinner) = &start_spinner {
                spinner.start("Waking up the local database");
            }
            let (url, mut child) = start_server(&destination, &package_name, template)?;
            if let Some(spinner) = &start_spinner {
                spinner.stop("Local app is ready");
            }
            println!("open {url}");
            if interactive {
                cliclack::outro(format!("Your local app is ready: {url}")).map_err(prompt_error)?;
            }
            println!("press Ctrl-C to stop");
            let status = child.wait().map_err(command_error)?;
            if !status.success() {
                return Err(CliError::Command(
                    "starter server stopped unexpectedly".to_owned(),
                ));
            }
        }
        Template::Minimal => {
            let start_spinner = interactive.then(cliclack::spinner);
            if let Some(spinner) = &start_spinner {
                spinner.start("Creating the local database");
            }
            run_minimal(&destination, &package_name)?;
            let projection = destination.join("data/projection.html");
            if let Some(spinner) = &start_spinner {
                spinner.stop("Database and projection ready");
            }
            println!("open file://{}", projection.display());
            if interactive {
                cliclack::outro(format!(
                    "Your data projection is ready: file://{}",
                    projection.display()
                ))
                .map_err(prompt_error)?;
            }
        }
    }
    Ok(())
}

fn scaffold(destination: &Path, package_name: &str, template: Template) -> Result<(), CliError> {
    if destination.exists() {
        return Err(CliError::Command(format!(
            "refusing to overwrite existing path `{}`",
            destination.display()
        )));
    }
    fs::create_dir_all(destination.join("src")).map_err(command_error)?;
    let (schema, main) = match template {
        Template::Notes => (NOTES_SCHEMA, NOTES_MAIN),
        Template::Ai => (AI_SCHEMA, AI_MAIN),
        Template::Topcoat => (TOPCOAT_SCHEMA, TOPCOAT_MAIN),
        Template::Minimal => (MINIMAL_SCHEMA, MINIMAL_MAIN),
    };
    write(destination.join("schema.sixpack"), schema)?;
    write(destination.join("src/main.rs"), main)?;
    write(
        destination.join("Cargo.toml"),
        &cargo_toml(package_name, template),
    )?;
    write(destination.join(".gitignore"), "/data\n/target\n")?;
    write(
        destination.join("README.md"),
        &starter_readme(package_name, template),
    )?;
    if template == Template::Topcoat {
        write(destination.join("Topcoat.toml"), TOPCOAT_CONFIG)?;
    }
    if matches!(template, Template::Notes | Template::Ai) {
        fs::create_dir_all(destination.join("static")).map_err(command_error)?;
        let html = match template {
            Template::Notes => NOTES_HTML,
            Template::Ai => AI_HTML,
            Template::Topcoat => unreachable!(),
            Template::Minimal => unreachable!(),
        };
        write(destination.join("static/index.html"), html)?;
    }
    Ok(())
}

fn cargo_toml(package_name: &str, template: Template) -> String {
    let sixpack = development_dependency();
    let extra = match template {
        Template::Notes | Template::Ai => {
            "serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n"
        }
        Template::Topcoat => {
            "serde = { version = \"1\", features = [\"derive\"] }\ntokio = { version = \"1\", features = [\"macros\", \"net\", \"rt-multi-thread\"] }\ntopcoat = \"=0.5.0\"\n"
        }
        Template::Minimal => "",
    };
    let dev = if template != Template::Minimal {
        "\n[dev-dependencies]\ntempfile = \"3\"\n"
    } else {
        ""
    };
    format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nsixpack = {sixpack}\n{extra}{dev}\n[workspace]\n"
    )
}

fn starter_readme(package_name: &str, template: Template) -> String {
    let details = match template {
        Template::Notes => {
            "Run `cargo run`, then open the printed local URL. The frontend is one plain HTML file under `static/`; the Rust binary serves it and the local CRUD API from one process. Database files and the generated read-only projection live under `data/`."
        }
        Template::Ai => {
            "Run `cargo run`, then open the printed local URL. Send `ping` in the chat; it writes both sides of the echoed exchange to sixpack so the compact database table updates beside the conversation."
        }
        Template::Topcoat => {
            "Install Topcoat CLI 0.5.0 with `cargo install topcoat-cli --version 0.5.0`, then run `topcoat dev` and open http://127.0.0.1:3000/. The single-page notes app is server-rendered from Topcoat `view!` components and persists form writes to the local Sixpack database under `data/`."
        }
        Template::Minimal => {
            "Run `cargo run`, then open `data/projection.html`. The Rust binary initializes the local database from `schema.sixpack` and refreshes the dependency-free projection."
        }
    };
    let run = if template == Template::Topcoat {
        "topcoat dev"
    } else {
        "cargo run"
    };
    format!(
        "# {package_name}\n\nGenerated by `sixpack create`.\n\n{details}\n\n```sh\n{run}\n```\n"
    )
}

fn development_dependency() -> String {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sixpack-cli has a packages parent")
        .join("sixpack");
    if package.join("Cargo.toml").is_file() {
        format!(
            "{{ path = {:?}, features = [\"experimental-compaction\"] }}",
            package
        )
    } else {
        format!("\"{}\"", env!("CARGO_PKG_VERSION"))
    }
}

fn build(destination: &Path) -> Result<(), CliError> {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--quiet")
        .current_dir(destination)
        .output()
        .map_err(command_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::Command(format!(
            "starter build failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn start_server(
    destination: &Path,
    package_name: &str,
    template: Template,
) -> Result<(String, std::process::Child), CliError> {
    let mut command = Command::new(binary_path(destination, package_name));
    match template {
        Template::Notes | Template::Ai => {
            command.args(["--database", "data", "--port", "0"]);
        }
        Template::Topcoat => {
            command.env("SIXPACK_DATABASE", "data").env("PORT", "0");
        }
        Template::Minimal => unreachable!(),
    }
    let mut child = command
        .current_dir(destination)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(command_error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Command("starter did not expose its output".to_owned()))?;
    let mut lines = BufReader::new(stdout).lines();
    let line = lines
        .next()
        .transpose()
        .map_err(command_error)?
        .ok_or_else(|| CliError::Command("starter exited before printing its URL".to_owned()))?;
    let prefix = match template {
        Template::Notes => "note-taking playground ",
        Template::Ai => "sixpack ai demo ",
        Template::Topcoat => "topcoat sixpack ",
        Template::Minimal => unreachable!(),
    };
    let url = line
        .strip_prefix(prefix)
        .map(str::to_owned)
        .ok_or_else(|| CliError::Command(format!("starter printed an unexpected line: {line}")))?;
    if template != Template::Topcoat {
        lines.next().transpose().map_err(command_error)?;
    }
    drop(lines);
    Ok((url, child))
}

fn run_minimal(destination: &Path, package_name: &str) -> Result<(), CliError> {
    let status = Command::new(binary_path(destination, package_name))
        .current_dir(destination)
        .stdout(Stdio::null())
        .status()
        .map_err(command_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Command("starter binary failed".to_owned()))
    }
}

fn binary_path(destination: &Path, package_name: &str) -> PathBuf {
    destination
        .join("target/debug")
        .join(format!("{package_name}{}", std::env::consts::EXE_SUFFIX))
}

fn package_name(destination: &Path) -> Result<String, CliError> {
    let raw = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::Usage("project path needs a final directory name".to_owned()))?;
    let mut name = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    if name.is_empty() || name.starts_with(|character: char| character.is_ascii_digit()) {
        name.insert_str(0, "sixpack-");
    }
    Ok(name)
}

fn write(path: PathBuf, contents: &str) -> Result<(), CliError> {
    fs::write(&path, contents).map_err(|error| {
        CliError::Command(format!("could not write `{}`: {error}", path.display()))
    })
}

fn command_error(error: std::io::Error) -> CliError {
    CliError::Command(error.to_string())
}

fn prompt_error(error: std::io::Error) -> CliError {
    CliError::Command(format!("terminal prompt failed: {error}"))
}

fn reject_remaining(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    if let Some(argument) = args.next() {
        Err(CliError::UnexpectedArgument(argument))
    } else {
        Ok(())
    }
}

fn print_logo(color: bool) {
    let logo = "  ███████╗██╗██╗  ██╗██████╗  █████╗  ██████╗██╗  ██╗\n  ██╔════╝██║╚██╗██╔╝██╔══██╗██╔══██╗██╔════╝██║ ██╔╝\n  ███████╗██║ ╚███╔╝ ██████╔╝███████║██║     █████╔╝\n  ╚════██║██║ ██╔██╗ ██╔═══╝ ██╔══██║██║     ██╔═██╗\n  ███████║██║██╔╝ ██╗██║     ██║  ██║╚██████╗██║  ██╗\n  ╚══════╝╚═╝╚═╝  ╚═╝╚═╝     ╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝";
    if color {
        println!("\x1b[38;2;202;255;79m{logo}\x1b[0m\n");
    } else {
        println!("{logo}\n");
    }
}

pub(crate) fn print_help() {
    println!("Create a runnable sixpack starter project.");
    println!();
    println!("Usage:");
    println!("  sixpack create [project] [--template notes|ai|topcoat|minimal] [--no-start]");
    println!();
    println!("Without arguments, the terminal asks for a path and template.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_template_contains_the_approved_app_and_rust_binary() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("notes-demo");
        scaffold(&destination, "notes-demo", Template::Notes).unwrap();

        assert!(destination.join("Cargo.toml").is_file());
        assert!(destination.join("schema.sixpack").is_file());
        assert!(destination.join("src/main.rs").is_file());
        assert!(destination.join("static/index.html").is_file());
        let html = fs::read_to_string(destination.join("static/index.html")).unwrap();
        assert!(html.contains("<h1>Notes</h1>"));
        assert!(html.contains("Database state"));
    }

    #[test]
    fn ai_template_contains_the_ping_endpoint_and_database_view() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("ai-demo");
        scaffold(&destination, "ai-demo", Template::Ai).unwrap();

        let source = fs::read_to_string(destination.join("src/main.rs")).unwrap();
        let html = fs::read_to_string(destination.join("static/index.html")).unwrap();
        assert!(source.contains("\"/api/ai/ping\""));
        assert!(source.contains("let reply = \"ping\""));
        assert!(html.contains("<h1>Chat app</h1>"));
        assert!(html.contains("Database state"));
    }

    #[test]
    fn topcoat_template_contains_a_compiled_page_and_project_marker() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("topcoat-demo");
        scaffold(&destination, "topcoat-demo", Template::Topcoat).unwrap();

        let manifest = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
        let source = fs::read_to_string(destination.join("src/main.rs")).unwrap();
        assert!(destination.join("Topcoat.toml").is_file());
        assert!(manifest.contains("topcoat = \"=0.5.0\""));
        assert!(source.contains("#[page(\"/\")]"));
        assert!(source.contains("<title>\"Topcoat Sixpack\"</title>"));
        assert!(source.contains("topcoat::serve(listener, router)"));
        assert!(!destination.join("static/index.html").exists());
    }

    #[test]
    fn scaffold_refuses_to_replace_an_existing_project() {
        let root = tempfile::tempdir().unwrap();
        let error = scaffold(root.path(), "existing", Template::Minimal).unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
    }
}
