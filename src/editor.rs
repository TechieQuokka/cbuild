//! Editor integration: teaching VS Code to flush its buffers before a build.
//!
//! `cman` cannot reach into an editor and save its unsaved work, so the next
//! best thing is to hand VS Code a setting that makes it save on its own the
//! moment focus leaves the window — which is exactly what happens when you
//! click over to a terminal to type `cman build`.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::commands::status;

/// Set this to any non-empty value to stop `cman` writing editor config.
pub const OPT_OUT_VAR: &str = "CMAN_NO_EDITOR_SETUP";

const VSCODE_DIR: &str = ".vscode";
const SETTINGS_FILE: &str = "settings.json";

pub const SETTINGS: &str = "{\n  \"files.autoSave\": \"onFocusChange\"\n}\n";

/// Drop a VS Code auto-save setting into `root`, unless the project already
/// has settings of its own.
///
/// Existing files are left strictly alone: VS Code settings are JSONC, so
/// merging a key into one without a JSONC parser risks mangling comments and
/// trailing commas that the editor accepts but a strict parser does not.
pub fn ensure_auto_save(root: &Path) -> Result<()> {
    if opted_out() {
        return Ok(());
    }
    let dir = root.join(VSCODE_DIR);
    let settings = dir.join(SETTINGS_FILE);
    if settings.exists() {
        return Ok(());
    }

    fs::create_dir_all(&dir).with_context(|| format!("failed to create `{}`", dir.display()))?;
    fs::write(&settings, SETTINGS)
        .with_context(|| format!("failed to write `{}`", settings.display()))?;

    // Announced once, on the run that creates it, so the appearance of an
    // untracked `.vscode/` directory is never a surprise.
    status("Configured", format!("{VSCODE_DIR}/{SETTINGS_FILE} (files.autoSave)"));
    Ok(())
}

fn opted_out() -> bool {
    env::var_os(OPT_OUT_VAR).is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{SETTINGS, ensure_auto_save};

    /// A temporary directory that cleans itself up, so the tests stay free of
    /// dev-dependencies.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cman-editor-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn settings(&self) -> std::path::PathBuf {
            self.0.join(".vscode").join("settings.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn writes_the_setting_when_the_project_has_none() {
        let dir = TempDir::new("fresh");

        ensure_auto_save(&dir.0).unwrap();

        assert_eq!(fs::read_to_string(dir.settings()).unwrap(), SETTINGS);
    }

    #[test]
    fn leaves_existing_settings_untouched() {
        let dir = TempDir::new("existing");
        let existing = "{\n  // hand-written\n  \"files.autoSave\": \"off\",\n}\n";
        fs::create_dir_all(dir.settings().parent().unwrap()).unwrap();
        fs::write(dir.settings(), existing).unwrap();

        ensure_auto_save(&dir.0).unwrap();

        assert_eq!(fs::read_to_string(dir.settings()).unwrap(), existing);
    }

    #[test]
    fn writing_twice_is_a_no_op() {
        let dir = TempDir::new("twice");

        ensure_auto_save(&dir.0).unwrap();
        ensure_auto_save(&dir.0).unwrap();

        assert_eq!(fs::read_to_string(dir.settings()).unwrap(), SETTINGS);
    }
}
