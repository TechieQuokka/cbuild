//! Command-line surface of `cman`.

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cman",
    version,
    about = "A Cargo-like build tool for C",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a new C project in a new directory
    New {
        /// Directory to create
        path: String,
        /// Package name (defaults to the directory name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Create a new C project in an existing directory
    Init {
        /// Directory to initialize (defaults to the current directory)
        path: Option<String>,
        /// Package name (defaults to the directory name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Compile the project
    Build {
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Build and run the project
    Run {
        #[command(flatten)]
        profile: ProfileArgs,
        /// Arguments passed through to the compiled program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Check the project for errors without producing any object code
    Check {
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Remove build artifacts
    Clean {
        /// Only remove the release artifacts
        #[arg(long)]
        release: bool,
    },
}

#[derive(Args)]
pub struct ProfileArgs {
    /// Build with optimizations
    #[arg(long)]
    pub release: bool,
}

/// Which set of compiler flags and which `target/` subdirectory to use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Dev,
    Release,
}

impl Profile {
    pub fn from_release_flag(release: bool) -> Self {
        if release { Self::Release } else { Self::Dev }
    }

    /// Name of the directory under `target/` that holds this profile's artifacts.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Dev => "debug",
            Self::Release => "release",
        }
    }

    /// Profile-specific compiler flags.
    pub fn flags(self) -> &'static [&'static str] {
        match self {
            Self::Dev => &["-g", "-O0"],
            Self::Release => &["-O2", "-DNDEBUG"],
        }
    }
}

impl From<&ProfileArgs> for Profile {
    fn from(args: &ProfileArgs) -> Self {
        Self::from_release_flag(args.release)
    }
}
