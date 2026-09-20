mod cli;
mod commands;
mod compiler;
mod fingerprint;
mod manifest;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command, Profile};

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => code,
        Err(error) => {
            // `{:#}` renders the whole anyhow context chain on one line.
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { path, name } => commands::new::new(&path, name)?,
        Command::Init { path, name } => commands::new::init(path.as_deref(), name)?,
        Command::Build { profile } => commands::build::execute(Profile::from(&profile))?,
        // `run` is the one command whose exit code is not ours to decide.
        Command::Run { profile, args } => {
            return commands::run::execute(Profile::from(&profile), &args);
        }
        Command::Check { profile } => commands::check::execute(Profile::from(&profile))?,
        Command::Clean { release } => commands::clean::execute(release)?,
    }
    Ok(ExitCode::SUCCESS)
}
