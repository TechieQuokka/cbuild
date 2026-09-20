pub mod build;
pub mod check;
pub mod clean;
pub mod new;
pub mod run;

/// Print a Cargo-style status line: a right-aligned verb followed by detail.
pub fn status(verb: &str, message: impl AsRef<str>) {
    println!("{:>12} {}", verb, message.as_ref());
}
