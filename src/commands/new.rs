//! `cman new` and `cman init` — scaffolding a project.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::commands::status;
use crate::editor;
use crate::manifest::MANIFEST_FILE;

const MAIN_C: &str = r#"#include <stdio.h>

int main(void) {
    printf("Hello, world!\n");
    return 0;
}
"#;

const GITIGNORE: &str = "target/\n";

/// Create the directory, then scaffold into it.
pub fn new(path: &str, name: Option<String>) -> Result<()> {
    let dir = absolute(Path::new(path))?;
    if dir.exists() && !is_empty(&dir)? {
        bail!("destination `{}` already exists and is not empty", dir.display());
    }
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create `{}`", dir.display()))?;
    scaffold(&dir, name)
}

/// Scaffold into an existing directory, leaving any sources already there
/// untouched.
pub fn init(path: Option<&str>, name: Option<String>) -> Result<()> {
    let dir = match path {
        Some(path) => absolute(Path::new(path))?,
        None => env::current_dir().context("failed to read the current directory")?,
    };
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create `{}`", dir.display()))?;
    if dir.join(MANIFEST_FILE).exists() {
        bail!("`{}` already exists in `{}`", MANIFEST_FILE, dir.display());
    }
    scaffold(&dir, name)
}

fn scaffold(dir: &Path, name: Option<String>) -> Result<()> {
    let name = match name {
        Some(name) => name,
        None => infer_name(dir)?,
    };
    validate_name(&name)?;

    write_new(&dir.join(MANIFEST_FILE), &manifest_template(&name))?;
    write_new(&dir.join(".gitignore"), GITIGNORE)?;

    let src = dir.join("src");
    fs::create_dir_all(&src).with_context(|| format!("failed to create `{}`", src.display()))?;
    write_new(&src.join("main.c"), MAIN_C)?;

    status("Created", format!("binary `{name}` package"));
    // After the "Created" line, so the editor note reads as a footnote to the
    // new package rather than as part of it.
    editor::ensure_auto_save(dir)?;
    Ok(())
}

fn manifest_template(name: &str) -> String {
    format!(
        "[package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         std = \"c17\"\n"
    )
}

/// Write a file only if it is not already there, so that `init` on a
/// directory with existing work never clobbers it.
fn write_new(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, contents).with_context(|| format!("failed to write `{}`", path.display()))
}

fn infer_name(dir: &Path) -> Result<String> {
    dir.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .with_context(|| {
            format!("cannot infer a package name from `{}`; pass --name", dir.display())
        })
}

/// Keep names to what can safely become a file name and a shell word, since
/// the package name is also the name of the produced executable.
fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        }
        _ => false,
    };
    if !valid {
        bail!(
            "invalid package name `{name}`: it must start with a letter or `_` \
             and contain only letters, digits, `_` or `-`"
        );
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().context("failed to read the current directory")?;
    Ok(cwd.join(path))
}

fn is_empty(dir: &Path) -> Result<bool> {
    if !dir.is_dir() {
        // An existing non-directory is definitely in the way.
        return Ok(false);
    }
    let mut entries =
        fs::read_dir(dir).with_context(|| format!("failed to read `{}`", dir.display()))?;
    Ok(entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::validate_name;

    #[test]
    fn accepts_ordinary_names() {
        assert!(validate_name("hello").is_ok());
        assert!(validate_name("my-app").is_ok());
        assert!(validate_name("_internal2").is_ok());
    }

    #[test]
    fn rejects_names_that_break_paths_or_shells() {
        assert!(validate_name("").is_err());
        assert!(validate_name("2fast").is_err());
        assert!(validate_name("-flag").is_err());
        assert!(validate_name("my app").is_err());
        assert!(validate_name("../escape").is_err());
    }
}
