//! Assembling compiler command lines and running them in parallel.

use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Context, Result};

use crate::cli::Profile;
use crate::manifest::Project;

/// Warnings every project gets, on the theory that a build tool should make
/// the compiler's opinion visible by default.
const DEFAULT_WARNINGS: &[&str] = &["-Wall", "-Wextra"];

pub struct Compiler {
    program: OsString,
}

impl Compiler {
    /// Honour `CC` if it is set, otherwise fall back to `cc`, which on this
    /// platform is a symlink to the system compiler.
    pub fn detect() -> Self {
        let program = env::var_os("CC")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("cc"));
        Self { program }
    }

    pub fn program(&self) -> &Path {
        Path::new(&self.program)
    }

    /// Flags shared by compiling, syntax checking and linking. These are the
    /// bytes that get hashed into the fingerprint, so anything that changes
    /// the meaning of a build has to end up in here.
    pub fn base_flags(&self, project: &Project, profile: Profile) -> Vec<String> {
        let mut flags = vec![format!("-std={}", project.manifest.package.std)];
        flags.extend(profile.flags().iter().map(|flag| flag.to_string()));
        flags.extend(DEFAULT_WARNINGS.iter().map(|flag| flag.to_string()));
        for dir in project.include_dirs() {
            flags.push(format!("-I{}", dir.display()));
        }
        flags
    }

    /// Compile one translation unit, emitting a `.d` sidecar that records
    /// which headers the object actually depends on.
    pub fn compile_job(
        &self,
        base_flags: &[String],
        src: &Path,
        obj: &Path,
        dep: &Path,
        label: String,
    ) -> Job {
        let mut args = base_flags.to_vec();
        args.push("-MMD".to_string());
        args.push("-MF".to_string());
        args.push(dep.display().to_string());
        args.push("-MT".to_string());
        args.push(obj.display().to_string());
        args.push("-c".to_string());
        args.push(src.display().to_string());
        args.push("-o".to_string());
        args.push(obj.display().to_string());
        Job::new(self.program(), args, label)
    }

    /// Parse and type-check a translation unit without writing any output.
    pub fn check_job(&self, base_flags: &[String], src: &Path, label: String) -> Job {
        let mut args = base_flags.to_vec();
        args.push("-fsyntax-only".to_string());
        args.push(src.display().to_string());
        Job::new(self.program(), args, label)
    }

    /// Link the objects into the final executable.
    pub fn link(&self, profile: Profile, objs: &[PathBuf], bin: &Path) -> Result<()> {
        let mut args: Vec<String> = profile.flags().iter().map(|flag| flag.to_string()).collect();
        args.extend(objs.iter().map(|obj| obj.display().to_string()));
        args.push("-o".to_string());
        args.push(bin.display().to_string());

        let job = Job::new(self.program(), args, format!("link {}", bin.display()));
        let outcome = job.run()?;
        outcome.emit();
        if !outcome.success {
            anyhow::bail!("linking failed");
        }
        Ok(())
    }
}

/// A single compiler invocation, kept as data so it can be queued and run on
/// a worker thread.
pub struct Job {
    program: PathBuf,
    args: Vec<String>,
    /// Human-readable description used in error messages.
    pub label: String,
}

impl Job {
    fn new(program: &Path, args: Vec<String>, label: String) -> Self {
        Self { program: program.to_path_buf(), args, label }
    }

    pub fn run(&self) -> Result<JobOutcome> {
        let output = Command::new(&self.program)
            .args(&self.args)
            .output()
            .with_context(|| format!("failed to run `{}`", self.program.display()))?;
        Ok(JobOutcome {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

pub struct JobOutcome {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl JobOutcome {
    /// Forward whatever the compiler said straight through, unmodified.
    /// Diagnostics are the compiler's job, not ours.
    pub fn emit(&self) {
        let _ = std::io::stdout().write_all(&self.stdout);
        let _ = std::io::stderr().write_all(&self.stderr);
    }
}

/// How many compiler processes to keep in flight.
pub fn parallelism() -> usize {
    thread::available_parallelism().map_or(1, |n| n.get())
}

/// Run every job, at most `workers` at a time, and return the outcomes in the
/// same order the jobs were given. Output is held until the end so that
/// diagnostics from concurrent compilations do not interleave.
pub fn run_jobs(jobs: &[Job], workers: usize) -> Result<Vec<JobOutcome>> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let next = AtomicUsize::new(0);
    let collected: Mutex<Vec<(usize, JobOutcome)>> = Mutex::new(Vec::with_capacity(jobs.len()));
    let failures: Mutex<Vec<anyhow::Error>> = Mutex::new(Vec::new());
    let workers = workers.clamp(1, jobs.len());

    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else { break };
                    match job.run() {
                        Ok(outcome) => collected.lock().unwrap().push((index, outcome)),
                        Err(error) => {
                            failures.lock().unwrap().push(error);
                            break;
                        }
                    }
                }
            });
        }
    });

    if let Some(error) = failures.into_inner().unwrap().pop() {
        return Err(error);
    }

    let mut collected = collected.into_inner().unwrap();
    collected.sort_by_key(|(index, _)| *index);
    Ok(collected.into_iter().map(|(_, outcome)| outcome).collect())
}
