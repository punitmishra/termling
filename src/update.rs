//! The `update` subcommand: bring termling up to date by delegating to whatever
//! installed it. **The binary makes no network calls itself** — it runs `brew`
//! or `cargo` (which do their own fetching), or, for a manual/tarball install,
//! prints the one-line installer. `update --check` reports the running version
//! and, where the package manager can say so cheaply, whether a newer one
//! exists. There is no automatic/background checking anywhere.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The version compiled into this binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "punitmishra/termling";
const INSTALLER: &str =
    "curl -fsSL https://raw.githubusercontent.com/punitmishra/termling/main/scripts/install.sh | sh";

/// How this binary was installed — determines how we update it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Install {
    Homebrew,
    Cargo,
    Manual,
}

/// Classify an executable path into an [`Install`] method.
fn detect_install(exe: &Path) -> Install {
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/") || s.contains("/homebrew/") {
        Install::Homebrew
    } else if s.contains("/.cargo/bin/") {
        Install::Cargo
    } else {
        Install::Manual
    }
}

/// The running binary, canonicalized so a Homebrew symlink resolves into its
/// Cellar path (which is what [`detect_install`] keys off).
fn current_exe() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("termling"));
    std::fs::canonicalize(&exe).unwrap_or(exe)
}

/// Entry point for `termling update [--check]`.
pub fn run(args: &[String]) -> io::Result<()> {
    let check_only = args.iter().any(|a| a == "--check");
    let install = detect_install(&current_exe());
    if check_only {
        check(install)
    } else {
        upgrade(install)
    }
}

fn upgrade(install: Install) -> io::Result<()> {
    println!("termling {VERSION}");
    match install {
        Install::Homebrew => {
            println!("updating via Homebrew…");
            run_cmd("brew", &["upgrade", "termling"])
        }
        Install::Cargo => {
            println!("updating via cargo…");
            run_cmd("cargo", &["install", "termling", "--force"])
        }
        Install::Manual => {
            println!("This looks like a manual install. Re-run the installer for the latest:");
            println!("  {INSTALLER}");
            println!("or grab a build from https://github.com/{REPO}/releases/latest");
            Ok(())
        }
    }
}

fn check(install: Install) -> io::Result<()> {
    println!("termling {VERSION} (installed)");
    match install {
        // Prints the formula + versions if outdated, nothing if current.
        Install::Homebrew => run_cmd("brew", &["outdated", "--verbose", "termling"]),
        Install::Cargo => {
            println!("run `cargo install termling` to pull the latest from crates.io");
            Ok(())
        }
        Install::Manual => {
            println!("latest release: https://github.com/{REPO}/releases/latest");
            Ok(())
        }
    }
}

/// Run an external command, inheriting its stdio, and turn a missing binary or a
/// non-zero exit into a friendly message rather than a hard error.
fn run_cmd(program: &str, args: &[&str]) -> io::Result<()> {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            eprintln!("termling: `{program}` exited with {status}");
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("termling: `{program}` not found on PATH — can't update this way.");
            eprintln!("  install the latest manually: {INSTALLER}");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_homebrew_paths() {
        assert_eq!(
            detect_install(Path::new(
                "/opt/homebrew/Cellar/termling/0.1.0/bin/termling"
            )),
            Install::Homebrew
        );
        assert_eq!(
            detect_install(Path::new("/usr/local/Cellar/termling/0.1.0/bin/termling")),
            Install::Homebrew
        );
        // An un-canonicalized brew symlink still lives under a homebrew prefix.
        assert_eq!(
            detect_install(Path::new("/opt/homebrew/bin/termling")),
            Install::Homebrew
        );
    }

    #[test]
    fn detects_cargo_path() {
        assert_eq!(
            detect_install(Path::new("/Users/me/.cargo/bin/termling")),
            Install::Cargo
        );
    }

    #[test]
    fn detects_manual_paths() {
        assert_eq!(
            detect_install(Path::new("/usr/local/bin/termling")),
            Install::Manual
        );
        assert_eq!(
            detect_install(Path::new("/Users/me/.local/bin/termling")),
            Install::Manual
        );
    }
}
