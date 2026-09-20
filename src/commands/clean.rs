//! `cman clean` — delete build artifacts.

use std::fs;

use anyhow::{Context, Result};

use crate::cli::Profile;
use crate::commands::{build, status};
use crate::manifest::Project;

pub fn execute(release: bool) -> Result<()> {
    let project = Project::discover()?;

    // Everything we remove lives under `target/`, which `cman` owns outright.
    let target = if release {
        project.profile_dir(Profile::Release)
    } else {
        project.target_dir()
    };

    if !target.exists() {
        status("Removed", "0 files");
        return Ok(());
    }

    let files = count_files(&target);
    fs::remove_dir_all(&target)
        .with_context(|| format!("failed to remove `{}`", target.display()))?;

    status(
        "Removed",
        format!("{} file(s) from `{}`", files, build::relative(&project, &target)),
    );
    Ok(())
}

/// Best-effort count for the status line; an unreadable subdirectory just
/// contributes nothing rather than failing the command.
fn count_files(dir: &std::path::Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => count_files(&entry.path()),
            _ => 1,
        })
        .sum()
}
