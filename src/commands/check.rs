//! `cman check` — compile far enough to find errors, then throw the result away.

use std::time::Instant;

use anyhow::{Result, bail};

use crate::cli::Profile;
use crate::commands::{build, status};
use crate::compiler::{self, Compiler};
use crate::manifest::Project;

pub fn execute(profile: Profile) -> Result<()> {
    let started = Instant::now();
    let project = Project::discover()?;

    let compiler = Compiler::detect();
    let base_flags = compiler.base_flags(&project, profile);

    status(
        "Checking",
        format!(
            "{} v{} ({})",
            project.name(),
            project.manifest.package.version,
            project.root.display()
        ),
    );

    // No fingerprinting here: `-fsyntax-only` writes nothing, so there is no
    // artifact whose freshness could be compared against the sources.
    let jobs: Vec<_> = project
        .sources()?
        .iter()
        .map(|src| compiler.check_job(&base_flags, src, build::relative(&project, src)))
        .collect();

    let outcomes = compiler::run_jobs(&jobs, compiler::parallelism())?;

    let mut failed = Vec::new();
    for (job, outcome) in jobs.iter().zip(&outcomes) {
        outcome.emit();
        if !outcome.success {
            failed.push(job.label.clone());
        }
    }

    if !failed.is_empty() {
        bail!(
            "could not check `{}` ({} of {} file(s) failed: {})",
            project.name(),
            failed.len(),
            jobs.len(),
            failed.join(", ")
        );
    }

    status(
        "Finished",
        format!(
            "`{}` profile target(s) in {:.2}s",
            profile.dir_name(),
            started.elapsed().as_secs_f64()
        ),
    );
    Ok(())
}
