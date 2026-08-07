//! Command-line layer for sixpack.
//!
//! This crate owns command parsing and CLI behavior.

mod bridge;
mod starter;

use std::fs;
use std::path::{Path, PathBuf};

use sixpack::{Database, write_data_projection};
use sixpack_schema_compiler::{
    compile_schema, database_schema_from_ir, emit_raw_rust, emit_typescript,
};

const DEFAULT_SCHEMA: &str = "schema.sixpack";
const DEFAULT_DATABASE: &str = "data";

/// Runs the command-line surface.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), CliError> {
    let current_dir = std::env::current_dir()
        .map_err(|error| CliError::Command(format!("could not read current directory: {error}")))?;
    run_from(&current_dir, args)
}

fn run_from(current_dir: &Path, args: impl IntoIterator<Item = String>) -> Result<(), CliError> {
    let mut args = args.into_iter();

    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            reject_extra_args(&mut args)?;
            println!("sixpack {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("help") => run_help(args),
        Some("--help") | Some("-h") | None => {
            reject_extra_args(&mut args)?;
            print_help();
            Ok(())
        }
        Some("init") => run_init(current_dir, args),
        Some("create") => starter::run(current_dir, args),
        Some("generate") => run_generate(current_dir, args),
        Some("project") => run_project(current_dir, args),
        Some("bridge") => bridge::run(args).map_err(|error| CliError::Bridge(error.to_string())),
        Some(command) => Err(CliError::UnknownCommand(command.to_owned())),
    }
}

fn run_help(mut args: impl Iterator<Item = String>) -> Result<(), CliError> {
    match args.next().as_deref() {
        None => print_help(),
        Some("init") => print_init_help(),
        Some("create") => starter::print_help(),
        Some("generate") => print_generate_help(),
        Some("project") => print_project_help(),
        Some(command) => {
            return Err(CliError::Usage(format!(
                "no help for `{command}`; run `sixpack help`"
            )));
        }
    }
    reject_extra_args(&mut args)
}

fn run_init(current_dir: &Path, args: impl Iterator<Item = String>) -> Result<(), CliError> {
    let mut schema = PathBuf::from(DEFAULT_SCHEMA);
    let mut database = PathBuf::from(DEFAULT_DATABASE);
    let mut database_was_set = false;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                reject_extra_args(&mut args)?;
                print_init_help();
                return Ok(());
            }
            "--schema" => {
                schema = PathBuf::from(required_value(&mut args, "--schema")?);
            }
            "--database" => {
                database = PathBuf::from(required_value(&mut args, "--database")?);
                database_was_set = true;
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnexpectedArgument(value.to_owned()));
            }
            value if !database_was_set => {
                database = PathBuf::from(value);
                database_was_set = true;
            }
            value => return Err(CliError::UnexpectedArgument(value.to_owned())),
        }
    }

    let schema_path = resolve_path(current_dir, &schema);
    let database_path = resolve_path(current_dir, &database);
    let source = read_schema(&schema_path)?;
    let ir = compile_schema(&source)
        .map_err(|error| CliError::Command(format!("schema compilation failed: {error}")))?;
    let runtime_schema = database_schema_from_ir(&ir)
        .map_err(|error| CliError::Command(format!("schema validation failed: {error}")))?;
    let db = Database::open_path_with_schema(&database_path, runtime_schema)
        .map_err(|error| CliError::Command(error.to_string()))?;
    db.init()
        .map_err(|error| CliError::Command(format!("database initialization failed: {error}")))?;
    let projection = db
        .write_projection()
        .map_err(|error| CliError::Command(format!("projection generation failed: {error}")))?;

    println!("initialized {}", database_path.display());
    println!("projection {}", projection.path.display());
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum GenerateTarget {
    Rust,
    TypeScript,
}

impl GenerateTarget {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "rust" => Ok(Self::Rust),
            "typescript" => Ok(Self::TypeScript),
            _ => Err(CliError::Usage(
                "usage: sixpack generate <rust|typescript> [schema.sixpack] [--out FILE]"
                    .to_owned(),
            )),
        }
    }
}

fn run_generate(
    current_dir: &Path,
    mut args: impl Iterator<Item = String>,
) -> Result<(), CliError> {
    let Some(target) = args.next() else {
        return Err(CliError::Usage(
            "usage: sixpack generate <rust|typescript> [schema.sixpack] [--out FILE]".to_owned(),
        ));
    };
    if matches!(target.as_str(), "-h" | "--help") {
        reject_extra_args(&mut args)?;
        print_generate_help();
        return Ok(());
    }
    let target = GenerateTarget::parse(&target)?;
    let mut schema = PathBuf::from(DEFAULT_SCHEMA);
    let mut schema_was_set = false;
    let mut output = None;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                reject_extra_args(&mut args)?;
                print_generate_help();
                return Ok(());
            }
            "--out" | "-o" => {
                output = Some(PathBuf::from(required_value(&mut args, "--out")?));
            }
            value if value.starts_with('-') => {
                return Err(CliError::UnexpectedArgument(value.to_owned()));
            }
            value if !schema_was_set => {
                schema = PathBuf::from(value);
                schema_was_set = true;
            }
            value => return Err(CliError::UnexpectedArgument(value.to_owned())),
        }
    }

    let schema_path = resolve_path(current_dir, &schema);
    let source = read_schema(&schema_path)?;
    let ir = compile_schema(&source)
        .map_err(|error| CliError::Command(format!("schema compilation failed: {error}")))?;
    let generated = match target {
        GenerateTarget::Rust => emit_raw_rust(&ir),
        GenerateTarget::TypeScript => emit_typescript(&ir),
    };

    if let Some(output) = output {
        let output = resolve_path(current_dir, &output);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                CliError::Command(format!(
                    "could not create output directory `{}`: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::write(&output, generated).map_err(|error| {
            CliError::Command(format!("could not write `{}`: {error}", output.display()))
        })?;
        println!("generated {}", output.display());
    } else {
        print!("{generated}");
    }
    Ok(())
}

fn run_project(current_dir: &Path, mut args: impl Iterator<Item = String>) -> Result<(), CliError> {
    let database = match args.next() {
        Some(argument) if matches!(argument.as_str(), "-h" | "--help") => {
            reject_extra_args(&mut args)?;
            print_project_help();
            return Ok(());
        }
        Some(argument) => PathBuf::from(argument),
        None => PathBuf::from(DEFAULT_DATABASE),
    };
    reject_extra_args(&mut args)?;
    let database = resolve_path(current_dir, &database);
    if !database.join("sixpack.toml").is_file() {
        return Err(CliError::Command(format!(
            "`{}` is not an initialized sixpack database",
            database.display()
        )));
    }
    let projection = write_data_projection(&database)
        .map_err(|error| CliError::Command(format!("projection generation failed: {error}")))?;
    println!("projection {}", projection.path.display());
    Ok(())
}

fn required_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, CliError> {
    args.next()
        .ok_or_else(|| CliError::Usage(format!("{option} requires a value")))
}

fn resolve_path(current_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        current_dir.join(path)
    }
}

fn read_schema(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path).map_err(|error| {
        CliError::Command(format!(
            "could not read schema `{}`: {error}",
            path.display()
        ))
    })
}

fn reject_extra_args(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    if let Some(argument) = args.next() {
        return Err(CliError::UnexpectedArgument(argument));
    }
    Ok(())
}

/// Command-line errors.
#[derive(Debug)]
pub enum CliError {
    /// The command is not recognized.
    UnknownCommand(String),
    /// An otherwise complete command received an extra argument.
    UnexpectedArgument(String),
    /// Command usage was invalid.
    Usage(String),
    /// A command failed while reading, validating, or writing local data.
    Command(String),
    /// The internal TypeScript bridge failed before it could return a response.
    Bridge(String),
}

impl CliError {
    /// Returns the intended process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UnknownCommand(_) | Self::UnexpectedArgument(_) | Self::Usage(_) => 2,
            Self::Command(_) | Self::Bridge(_) => 1,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(command) => {
                writeln!(formatter, "unknown command: {command}")?;
                write!(formatter, "run `sixpack help` for usage")
            }
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
            Self::Usage(message) | Self::Command(message) | Self::Bridge(message) => {
                write!(formatter, "{message}")
            }
        }
    }
}

impl std::error::Error for CliError {}

fn print_help() {
    println!("sixpack");
    println!();
    println!("Usage:");
    println!("  sixpack init [database] [--schema schema.sixpack]");
    println!("  sixpack create [project] [--template notes|ai|topcoat]");
    println!("  sixpack generate <rust|typescript> [schema.sixpack] [--out FILE]");
    println!("  sixpack project [database]");
    println!("  sixpack help [command]");
    println!("  sixpack --version");
    println!();
    println!("Defaults: schema.sixpack and ./data");
}

fn print_init_help() {
    println!("Initialize a local database and its projection.");
    println!();
    println!("Usage:");
    println!("  sixpack init [database] [--schema schema.sixpack]");
    println!();
    println!("Default database: ./data");
}

fn print_generate_help() {
    println!("Generate a typed schema module.");
    println!();
    println!("Usage:");
    println!("  sixpack generate <rust|typescript> [schema.sixpack] [--out FILE]");
    println!();
    println!("Without --out, generated source is written to stdout.");
}

fn print_project_help() {
    println!("Regenerate the read-only HTML projection.");
    println!();
    println!("Usage:");
    println!("  sixpack project [database]");
    println!();
    println!("Default database: ./data");
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = concat!(
        "schema! {\n",
        "  notes {\n",
        "    id id\n",
        "    title text\n",
        "    lookup title unique\n",
        "  }\n",
        "}\n",
    );

    #[test]
    fn init_uses_ergonomic_defaults_and_generates_projection() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(DEFAULT_SCHEMA), SCHEMA).unwrap();

        run_from(root.path(), ["init".to_owned()]).unwrap();

        assert!(root.path().join("data/sixpack.toml").is_file());
        assert!(root.path().join("data/projection.html").is_file());
        assert!(root.path().join("data/tables/notes").is_dir());
        assert!(root.path().join("data/engine/notes.6b").is_file());
    }

    #[test]
    fn rust_generation_supports_an_output_file() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(DEFAULT_SCHEMA), SCHEMA).unwrap();

        run_from(
            root.path(),
            [
                "generate".to_owned(),
                "rust".to_owned(),
                "--out".to_owned(),
                "generated/schema.rs".to_owned(),
            ],
        )
        .unwrap();

        let output = fs::read_to_string(root.path().join("generated/schema.rs")).unwrap();
        assert!(output.contains("pub mod notes"));
        assert!(output.contains("pub fn database_schema"));
    }

    #[test]
    fn project_rejects_an_uninitialized_directory() {
        let root = tempfile::tempdir().unwrap();
        let error =
            run_from(root.path(), ["project".to_owned(), "missing".to_owned()]).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("not an initialized"));
    }

    #[test]
    fn generate_requires_a_target() {
        let root = tempfile::tempdir().unwrap();
        let error = run_from(root.path(), ["generate".to_owned()]).unwrap_err();

        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("<rust|typescript>"));
    }
}
