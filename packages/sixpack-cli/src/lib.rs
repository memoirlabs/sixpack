//! Command-line layer for sixpack.
//!
//! This crate owns command parsing and CLI behavior.

mod bridge;

use std::fs;

use sixpack_schema_compiler::{compile_schema, emit_typescript};

/// Runs the command-line surface.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), CliError> {
    let mut args = args.into_iter();

    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            reject_extra_args(&mut args)?;
            println!("sixpack {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            reject_extra_args(&mut args)?;
            print_help();
            Ok(())
        }
        Some("generate") => run_generate(args),
        Some("bridge") => bridge::run(args).map_err(|error| CliError::Bridge(error.to_string())),
        Some(command) => Err(CliError::UnknownCommand(command.to_owned())),
    }
}

fn run_generate(mut args: impl Iterator<Item = String>) -> Result<(), CliError> {
    match (args.next().as_deref(), args.next()) {
        (Some("typescript"), Some(schema_path)) => {
            reject_extra_args(&mut args)?;
            let source = fs::read_to_string(&schema_path).map_err(|error| {
                CliError::Generate(format!("could not read schema `{schema_path}`: {error}"))
            })?;
            let ir = compile_schema(&source).map_err(|error| {
                CliError::Generate(format!("schema compilation failed: {error}"))
            })?;
            print!("{}", emit_typescript(&ir));
            Ok(())
        }
        _ => Err(CliError::Usage(
            "usage: sixpack generate typescript <schema.sixpack>".to_owned(),
        )),
    }
}

fn reject_extra_args(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    if let Some(argument) = args.next() {
        return Err(CliError::UnexpectedArgument(argument));
    }
    Ok(())
}

/// Command-line errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The command is not recognized.
    UnknownCommand(String),
    /// An otherwise complete command received an extra argument.
    UnexpectedArgument(String),
    /// Command usage was invalid.
    Usage(String),
    /// Schema generation failed.
    Generate(String),
    /// The internal TypeScript bridge failed before it could return a response.
    Bridge(String),
}

impl CliError {
    /// Returns the intended process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UnknownCommand(_) => 2,
            Self::UnexpectedArgument(_) | Self::Usage(_) => 2,
            Self::Generate(_) | Self::Bridge(_) => 1,
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
            Self::Usage(message) | Self::Generate(message) => write!(formatter, "{message}"),
            Self::Bridge(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for CliError {}

fn print_help() {
    println!("sixpack");
    println!();
    println!("Usage:");
    println!("  sixpack --version");
    println!("  sixpack help");
    println!("  sixpack generate typescript <schema.sixpack>");
}
