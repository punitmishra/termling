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

/// Classify an executable path into an [`Install`] method. `cargo_bins` is the
/// set of `cargo install` target dirs to check (from `$CARGO_INSTALL_ROOT`,
/// `$CARGO_HOME`, and the `~/.cargo/bin` default) — passed in so this stays a
/// pure function. We match whole path *components*, not substrings: a substring
/// test flags a manual install under, say, `~/homebrew-notes/` as Homebrew, and
/// misses a Cargo install whose `$CARGO_HOME` isn't `~/.cargo` (finding #11).
fn detect_install(exe: &Path, cargo_bins: &[PathBuf]) -> Install {
    if is_homebrew(exe) {
        Install::Homebrew
    } else if cargo_bins.iter().any(|b| exe.starts_with(b)) {
        Install::Cargo
    } else {
        Install::Manual
    }
}

/// True if any path component marks a Homebrew tree — `Cellar` (present in every
/// canonicalized install, any prefix) or the `homebrew`/`.linuxbrew` prefix dir
/// (for an un-canonicalized `/opt/homebrew/bin/termling` symlink).
fn is_homebrew(exe: &Path) -> bool {
    exe.components().any(|c| {
        let c = c.as_os_str();
        c == "Cellar" || c == "homebrew" || c == ".linuxbrew"
    })
}

/// The `cargo install` bin dirs to recognize, in the order cargo itself would
/// use one: `$CARGO_INSTALL_ROOT/bin`, `$CARGO_HOME/bin`, then `~/.cargo/bin`.
fn cargo_bins() -> Vec<PathBuf> {
    let mut bins = Vec::new();
    if let Some(root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        bins.push(Path::new(&root).join("bin"));
    }
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        bins.push(Path::new(&home).join("bin"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        bins.push(Path::new(&home).join(".cargo").join("bin"));
    }
    bins
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
    let install = detect_install(&current_exe(), &cargo_bins());
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

    /// The default `~/.cargo/bin` for a fixed HOME, as tests would see it.
    fn default_cargo_bins() -> Vec<PathBuf> {
        vec![PathBuf::from("/Users/me/.cargo/bin")]
    }

    #[test]
    fn detects_homebrew_paths() {
        let cb = default_cargo_bins();
        assert_eq!(
            detect_install(
                Path::new("/opt/homebrew/Cellar/termling/0.1.0/bin/termling"),
                &cb
            ),
            Install::Homebrew
        );
        assert_eq!(
            detect_install(
                Path::new("/usr/local/Cellar/termling/0.1.0/bin/termling"),
                &cb
            ),
            Install::Homebrew
        );
        // An un-canonicalized brew symlink still lives under a homebrew prefix.
        assert_eq!(
            detect_install(Path::new("/opt/homebrew/bin/termling"), &cb),
            Install::Homebrew
        );
        // Linux Homebrew.
        assert_eq!(
            detect_install(
                Path::new("/home/linuxbrew/.linuxbrew/Cellar/termling/0.1.0/bin/termling"),
                &cb
            ),
            Install::Homebrew
        );
    }

    #[test]
    fn detects_cargo_path() {
        assert_eq!(
            detect_install(
                Path::new("/Users/me/.cargo/bin/termling"),
                &default_cargo_bins()
            ),
            Install::Cargo
        );
        // A non-default CARGO_HOME / CARGO_INSTALL_ROOT is still Cargo.
        let cb = vec![PathBuf::from("/opt/cargo/bin")];
        assert_eq!(
            detect_install(Path::new("/opt/cargo/bin/termling"), &cb),
            Install::Cargo
        );
    }

    #[test]
    fn detects_manual_paths() {
        let cb = default_cargo_bins();
        assert_eq!(
            detect_install(Path::new("/usr/local/bin/termling"), &cb),
            Install::Manual
        );
        assert_eq!(
            detect_install(Path::new("/Users/me/.local/bin/termling"), &cb),
            Install::Manual
        );
        // A manual install under a dir that merely *contains* the substring
        // "homebrew" is NOT Homebrew — the old substring check got this wrong.
        assert_eq!(
            detect_install(Path::new("/Users/me/homebrew-notes/bin/termling"), &cb),
            Install::Manual
        );
    }
}
