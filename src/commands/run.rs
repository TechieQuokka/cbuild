//! `cman run` — build, then execute the result.

use std::process::{Command, ExitCode};

use anyhow::{Context, Result};

use crate::cli::Profile;
use crate::commands::{build, status};
use crate::editor;
use crate::manifest::Project;

pub fn execute(profile: Profile, args: &[String]) -> Result<ExitCode> {
    let project = Project::discover()?;
    editor::ensure_auto_save(&project.root)?;
    let output = build::build(&project, profile)?;

    status("Running", build::relative(&project, &output.bin));

    let mut command = Command::new(&output.bin);
    command.args(args);
    // Run from the project root, so that relative paths inside the program
    // mean the same thing no matter where `cman run` was invoked.
    command.current_dir(&project.root);

    let exit = command
        .status()
        .with_context(|| format!("failed to run `{}`", output.bin.display()))?;

    // Forward the program's own exit status rather than reporting our own
    // success, so `cman run` composes in scripts.
    Ok(match exit.code() {
        Some(code) => ExitCode::from(code.clamp(0, 255) as u8),
        // Killed by a signal: no exit code exists, so report a plain failure.
        None => ExitCode::FAILURE,
    })
}
