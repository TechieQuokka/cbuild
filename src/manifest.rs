//! Parsing of `cman.toml` and discovery of the project it belongs to.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::cli::Profile;

/// The manifest file name, the marker that identifies a project root.
pub const MANIFEST_FILE: &str = "cman.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub package: Package,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// C standard passed to the compiler as `-std=<std>`.
    #[serde(default = "default_std")]
    pub std: String,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_std() -> String {
    "c17".to_string()
}

impl Manifest {
    fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("failed to parse manifest")
    }
}

/// A located project: its root directory plus the parsed manifest.
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Project {
    /// Find the project by walking up from the current directory, the way
    /// `cargo` locates `Cargo.toml`.
    pub fn discover() -> Result<Self> {
        let cwd = env::current_dir().context("failed to read the current directory")?;
        let root = find_root(&cwd).with_context(|| {
            format!("could not find `{MANIFEST_FILE}` in `{}` or any parent directory", cwd.display())
        })?;
        Self::load(root)
    }

    fn load(root: PathBuf) -> Result<Self> {
        let path = root.join(MANIFEST_FILE);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read `{}`", path.display()))?;
        let manifest = Manifest::parse(&text)
            .with_context(|| format!("invalid manifest `{}`", path.display()))?;
        if manifest.package.name.is_empty() {
            bail!("`package.name` in `{}` must not be empty", path.display());
        }
        Ok(Self { root, manifest })
    }

    pub fn name(&self) -> &str {
        &self.manifest.package.name
    }

    pub fn src_dir(&self) -> PathBuf {
        self.root.join("src")
    }

    /// Public header directory. Optional: absent in projects that keep every
    /// header next to its `.c` file.
    pub fn include_dir(&self) -> PathBuf {
        self.root.join("include")
    }

    pub fn target_dir(&self) -> PathBuf {
        self.root.join("target")
    }

    /// `target/debug` or `target/release`. Keeping the profiles in separate
    /// trees means switching `--release` never mixes objects built with
    /// different flags.
    pub fn profile_dir(&self, profile: Profile) -> PathBuf {
        self.target_dir().join(profile.dir_name())
    }

    pub fn obj_dir(&self, profile: Profile) -> PathBuf {
        self.profile_dir(profile).join("obj")
    }

    pub fn bin_path(&self, profile: Profile) -> PathBuf {
        self.profile_dir(profile).join(self.name())
    }

    /// Directories searched for headers, passed to the compiler as `-I`.
    pub fn include_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let include = self.include_dir();
        if include.is_dir() {
            dirs.push(include);
        }
        dirs.push(self.src_dir());
        dirs
    }

    /// Every `.c` file under `src/`, sorted so that build output is stable
    /// from run to run.
    pub fn sources(&self) -> Result<Vec<PathBuf>> {
        let src = self.src_dir();
        if !src.is_dir() {
            bail!("no `src` directory in `{}`", self.root.display());
        }
        let mut sources = Vec::new();
        collect_sources(&src, &mut sources)?;
        if sources.is_empty() {
            bail!("no `.c` files found under `{}`", src.display());
        }
        sources.sort();
        Ok(sources)
    }
}

fn find_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(MANIFEST_FILE).is_file())
        .map(Path::to_path_buf)
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("failed to read `{}`", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read `{}`", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat `{}`", path.display()))?;
        if file_type.is_dir() {
            collect_sources(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "c") {
            out.push(path);
        }
    }
    Ok(())
}
