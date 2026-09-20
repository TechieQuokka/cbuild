//! Deciding what actually needs to be rebuilt.
//!
//! Two independent signals feed the decision:
//!
//! * per-object freshness, derived from the `.d` files the compiler writes,
//!   which is what makes editing a header trigger a rebuild of everything
//!   that includes it;
//! * a hash of the compiler flags for the whole profile, which catches the
//!   cases mtimes cannot see, such as editing `std` in the manifest.

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

/// File under `target/<profile>/` holding the flag hash of the last build.
const FINGERPRINT_FILE: &str = ".cman-fingerprint";

/// Hash of everything that would change the meaning of a compilation.
pub fn flags_hash(program: &Path, flags: &[String]) -> String {
    let mut hasher = DefaultHasher::new();
    program.hash(&mut hasher);
    flags.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn fingerprint_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(FINGERPRINT_FILE)
}

/// Whether the recorded flag hash differs from the current one. A missing or
/// unreadable record counts as changed, which forces a full rebuild.
pub fn flags_changed(profile_dir: &Path, hash: &str) -> bool {
    match fs::read_to_string(fingerprint_path(profile_dir)) {
        Ok(recorded) => recorded.trim() != hash,
        Err(_) => true,
    }
}

pub fn record_flags(profile_dir: &Path, hash: &str) -> Result<()> {
    let path = fingerprint_path(profile_dir);
    fs::write(&path, hash).with_context(|| format!("failed to write `{}`", path.display()))
}

/// Whether `src` has to be recompiled into `obj`.
///
/// `forced` short-circuits the check for the flag-change case, so that the
/// caller does not have to duplicate the logic per file.
pub fn needs_rebuild(src: &Path, obj: &Path, dep: &Path, forced: bool) -> Result<bool> {
    if forced {
        return Ok(true);
    }
    let Some(obj_time) = modified(obj)? else {
        return Ok(true);
    };
    if is_newer_than(src, obj_time)? {
        return Ok(true);
    }
    // No dependency record means we cannot know which headers matter, so the
    // only safe answer is to rebuild and produce one.
    let Ok(text) = fs::read_to_string(dep) else {
        return Ok(true);
    };
    for header in parse_make_deps(&text) {
        if is_newer_than(&header, obj_time)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the executable is out of date with respect to its objects.
pub fn needs_relink(objs: &[PathBuf], bin: &Path) -> Result<bool> {
    let Some(bin_time) = modified(bin)? else {
        return Ok(true);
    };
    for obj in objs {
        if is_newer_than(obj, bin_time)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A missing file reads as "newer", so that a deleted header or object
/// triggers the rebuild that will report the real problem.
fn is_newer_than(path: &Path, reference: SystemTime) -> Result<bool> {
    Ok(match modified(path)? {
        Some(time) => time > reference,
        None => true,
    })
}

fn modified(path: &Path) -> Result<Option<SystemTime>> {
    match fs::metadata(path) {
        Ok(metadata) => {
            let time = metadata
                .modified()
                .with_context(|| format!("failed to read the mtime of `{}`", path.display()))?;
            Ok(Some(time))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(anyhow::Error::from(error).context(format!("failed to stat `{}`", path.display())))
        }
    }
}

/// Extract the prerequisites from a makefile-syntax dependency file, i.e. the
/// paths after the first `:`. Handles the two escapes GCC emits: `\` before a
/// newline to continue a line, and `\` before a space inside a path.
fn parse_make_deps(text: &str) -> Vec<PathBuf> {
    let mut deps = Vec::new();
    let mut token = String::new();
    let mut past_target = false;
    let mut chars = text.chars().peekable();

    let flush = |token: &mut String, deps: &mut Vec<PathBuf>, past_target: bool| {
        if !token.is_empty() {
            if past_target {
                deps.push(PathBuf::from(token.as_str()));
            }
            token.clear();
        }
    };

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek() {
                // A continued line separates tokens just like a space does.
                Some('\n') | Some('\r') => {
                    chars.next();
                    flush(&mut token, &mut deps, past_target);
                }
                Some(_) => {
                    let escaped = chars.next().expect("peeked");
                    token.push(escaped);
                }
                None => {}
            },
            ':' if !past_target => {
                // Everything up to here was the target, not a dependency.
                token.clear();
                past_target = true;
            }
            ch if ch.is_whitespace() => flush(&mut token, &mut deps, past_target),
            ch => token.push(ch),
        }
    }
    flush(&mut token, &mut deps, past_target);
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_line() {
        let deps = parse_make_deps("target/debug/obj/main.o: src/main.c include/hello.h\n");
        assert_eq!(deps, vec![PathBuf::from("src/main.c"), PathBuf::from("include/hello.h")]);
    }

    #[test]
    fn parses_continued_lines() {
        let deps = parse_make_deps("main.o: src/main.c \\\n  include/a.h \\\n  src/b.h\n");
        assert_eq!(
            deps,
            vec![
                PathBuf::from("src/main.c"),
                PathBuf::from("include/a.h"),
                PathBuf::from("src/b.h"),
            ]
        );
    }

    #[test]
    fn parses_escaped_spaces_in_paths() {
        let deps = parse_make_deps(r"main.o: src/my\ file.c");
        assert_eq!(deps, vec![PathBuf::from("src/my file.c")]);
    }

    #[test]
    fn yields_nothing_without_a_target() {
        assert!(parse_make_deps("").is_empty());
        assert!(parse_make_deps("src/main.c include/hello.h").is_empty());
    }
}
