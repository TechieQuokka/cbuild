//! `cman build` — the command everything else is built on top of.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::cli::Profile;
use crate::commands::status;
use crate::compiler::{self, Compiler};
use crate::fingerprint;
use crate::manifest::Project;

/// One translation unit and the artifacts derived from it.
struct Unit {
    src: PathBuf,
    obj: PathBuf,
    dep: PathBuf,
}

pub struct BuildOutput {
    pub bin: PathBuf,
}

pub fn execute(profile: Profile) -> Result<()> {
    let project = Project::discover()?;
    build(&project, profile).map(|_| ())
}

/// Bring `target/<profile>/<name>` up to date and return where it is.
pub fn build(project: &Project, profile: Profile) -> Result<BuildOutput> {
    let started = Instant::now();

    let compiler = Compiler::detect();
    let base_flags = compiler.base_flags(project, profile);
    let hash = fingerprint::flags_hash(compiler.program(), &base_flags);

    let profile_dir = project.profile_dir(profile);
    fs::create_dir_all(&profile_dir)
        .with_context(|| format!("failed to create `{}`", profile_dir.display()))?;

    // A flag change invalidates every object, because mtimes alone cannot
    // tell that `-O0` became `-O2` or that `std` moved in the manifest.
    let forced = fingerprint::flags_changed(&profile_dir, &hash);

    let units = plan(project, profile)?;
    let objs: Vec<PathBuf> = units.iter().map(|unit| unit.obj.clone()).collect();
    let bin = project.bin_path(profile);

    let mut stale = Vec::new();
    for unit in &units {
        if fingerprint::needs_rebuild(&unit.src, &unit.obj, &unit.dep, forced)? {
            stale.push(unit);
        }
    }

    // Relinking is also the signal that this build is doing any work at all:
    // compiling anything implies it, and it covers a deleted or stale binary.
    let relink = !stale.is_empty() || fingerprint::needs_relink(&objs, &bin)?;
    if relink {
        status("Compiling", describe(project));
    }

    if !stale.is_empty() {
        compile(project, &compiler, &base_flags, &stale)?;
    }

    if relink {
        // Same reasoning as for objects: a partially written executable with
        // a fresh mtime would look up to date on the next build.
        remove_if_present(&bin)?;
        compiler.link(profile, &objs, &bin)?;
    }

    fingerprint::record_flags(&profile_dir, &hash)?;

    status(
        "Finished",
        format!(
            "`{}` profile [{}] target(s) in {:.2}s",
            profile.dir_name(),
            describe_profile(profile),
            started.elapsed().as_secs_f64()
        ),
    );

    Ok(BuildOutput { bin })
}

fn compile(
    project: &Project,
    compiler: &Compiler,
    base_flags: &[String],
    stale: &[&Unit],
) -> Result<()> {
    let mut jobs = Vec::with_capacity(stale.len());
    for unit in stale {
        if let Some(parent) = unit.obj.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        // Drop the previous artifacts first. If the compiler fails partway
        // through, the absence of the object is what guarantees the next
        // build retries it; leaving a half-written one behind would let a
        // fresh mtime disguise a corrupt object as up to date.
        remove_if_present(&unit.obj)?;
        remove_if_present(&unit.dep)?;

        let label = relative(project, &unit.src);
        jobs.push(compiler.compile_job(base_flags, &unit.src, &unit.obj, &unit.dep, label));
    }

    let outcomes = compiler::run_jobs(&jobs, compiler::parallelism())?;

    // Emit in job order rather than completion order so that output is the
    // same whether or not the build ran in parallel.
    let mut failed = Vec::new();
    for (job, outcome) in jobs.iter().zip(&outcomes) {
        outcome.emit();
        if !outcome.success {
            failed.push(job.label.clone());
        }
    }

    if !failed.is_empty() {
        bail!(
            "could not compile `{}` ({} of {} file(s) failed: {})",
            project.name(),
            failed.len(),
            jobs.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

/// Map every source file to the object and dependency file it produces,
/// mirroring the `src/` tree under `obj/` so nested files cannot collide.
fn plan(project: &Project, profile: Profile) -> Result<Vec<Unit>> {
    let src_dir = project.src_dir();
    let obj_dir = project.obj_dir(profile);

    project
        .sources()?
        .into_iter()
        .map(|src| {
            let relative = src
                .strip_prefix(&src_dir)
                .with_context(|| format!("`{}` is outside `{}`", src.display(), src_dir.display()))?;
            let mut obj = obj_dir.join(relative);
            obj.set_extension("o");
            let mut dep = obj.clone();
            dep.set_extension("d");
            Ok(Unit { src, obj, dep })
        })
        .collect()
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::from(error)
            .context(format!("failed to remove `{}`", path.display()))),
    }
}

fn describe(project: &Project) -> String {
    format!(
        "{} v{} ({})",
        project.name(),
        project.manifest.package.version,
        project.root.display()
    )
}

fn describe_profile(profile: Profile) -> &'static str {
    match profile {
        Profile::Dev => "unoptimized + debuginfo",
        Profile::Release => "optimized",
    }
}

/// Shorten a path for display by making it relative to the project root.
pub fn relative(project: &Project, path: &Path) -> String {
    path.strip_prefix(&project.root)
        .unwrap_or(path)
        .display()
        .to_string()
}
