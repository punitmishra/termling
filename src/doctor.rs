//! The `doctor` subcommand: a quick, network-free environment check that
//! answers "will the companion work here, and if it didn't spawn, why?". It
//! reuses the same scope/socket/liveness logic the real companion uses, so what
//! it reports is exactly what `ensure`/`tmux` see.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use crate::event;

/// A minimal, testable view of the terminal-identifying environment.
struct EnvView {
    tmux: bool,
    iterm: bool,
    kitty: bool,
    wezterm: bool,
}

impl EnvView {
    fn from_env() -> Self {
        EnvView {
            tmux: std::env::var_os("TMUX").is_some(),
            iterm: std::env::var_os("ITERM_SESSION_ID").is_some(),
            kitty: std::env::var_os("KITTY_WINDOW_ID").is_some(),
            wezterm: std::env::var_os("WEZTERM_PANE").is_some(),
        }
    }
}

/// Which window manager, if any, can auto-split a companion here. Order matches
/// `tmux::spawn_companion` so the report reflects what will actually happen.
fn autospawn_support(e: &EnvView) -> Option<&'static str> {
    if e.tmux {
        Some("tmux")
    } else if e.iterm {
        Some("iTerm2")
    } else if e.kitty {
        Some("kitty")
    } else if e.wezterm {
        Some("WezTerm")
    } else {
        None
    }
}

/// Is the running binary reachable by name through `$PATH` (so the hook's bare
/// `termling` resolves to it)?
fn on_path() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(name) = exe.file_name() else {
        return false;
    };
    resolves_on_path(&exe, std::env::var_os("PATH").as_deref(), name)
}

/// Pure core of [`on_path`]: does some `$PATH` entry hold a file named
/// `file_name` that resolves to the same binary as `exe`? We compare the
/// *canonical* paths, so a symlinked install — e.g. Homebrew's `bin` dir
/// linking into `Cellar` — still counts. A plain "is the parent dir literally
/// in PATH" check misses that and reports a false "not on PATH" (findings
/// #9/#16), which bites Linux/brew users specifically.
fn resolves_on_path(exe: &Path, path_var: Option<&OsStr>, file_name: &OsStr) -> bool {
    let Some(path) = path_var else {
        return false;
    };
    let target = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    std::env::split_paths(path)
        .any(|dir| std::fs::canonicalize(dir.join(file_name)).is_ok_and(|real| real == target))
}

fn check(ok: bool, yes: &str, no: &str) {
    if ok {
        println!("  ✓ {yes}");
    } else {
        println!("  ✗ {no}");
    }
}

/// Entry point for `termling doctor`.
pub fn run() -> io::Result<()> {
    let scope = event::scope();
    let socket = event::socket_path(&scope);

    println!("termling {}", crate::update::VERSION);
    println!();
    println!("scope        {scope}");
    println!("socket       {}", socket.display());
    println!();

    check(
        event::is_live(&scope),
        "a companion is running for this scope",
        "no companion for this scope — open a shell with the hook, or run `termling tmux`",
    );

    match autospawn_support(&EnvView::from_env()) {
        // kitty splits only work when its remote-control socket is on; say so
        // rather than promise a spawn that may silently no-op (finding #12).
        Some("kitty") => println!(
            "  ✓ auto-spawn supported via kitty (requires `allow_remote_control yes` in kitty.conf)"
        ),
        Some(term) => println!("  ✓ auto-spawn supported via {term}"),
        None => println!(
            "  ✗ no split-capable terminal detected — open a second pane and run \
             `termling`, or use `termling tmux`"
        ),
    }

    let autospawn_on = std::env::var("TERMLING_AUTOSPAWN").as_deref() != Ok("0");
    check(
        autospawn_on,
        "auto-spawn enabled",
        "auto-spawn disabled (TERMLING_AUTOSPAWN=0) — unset it to spawn automatically",
    );

    check(
        on_path(),
        "binary is on your PATH",
        "binary not on PATH — add its directory so the hook's `termling` resolves",
    );

    let privacy = std::env::var("TERMLING_PRIVACY").unwrap_or_else(|_| "name (default)".into());
    println!("  · privacy: {privacy}");

    println!();
    println!("hook — add this to your shell rc, then open a new window:");
    let line = match crate::init::detect_shell() {
        Some("fish") => "termling init fish | source",
        Some("bash") => "eval \"$(termling init bash)\"",
        Some("zsh") => "eval \"$(termling init zsh)\"",
        // $SHELL isn't one we recognize — show the form and let them pick.
        _ => "eval \"$(termling init zsh)\"   # or: bash | fish",
    };
    println!("  {line}");
    println!("(the hook can't be inspected from here — after installing, run any");
    println!(" command and watch the companion react.)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(tmux: bool, iterm: bool, kitty: bool, wezterm: bool) -> EnvView {
        EnvView {
            tmux,
            iterm,
            kitty,
            wezterm,
        }
    }

    #[test]
    fn autospawn_support_follows_spawn_precedence() {
        // tmux wins even when several are present.
        assert_eq!(
            autospawn_support(&view(true, true, true, true)),
            Some("tmux")
        );
        assert_eq!(
            autospawn_support(&view(false, true, false, false)),
            Some("iTerm2")
        );
        assert_eq!(
            autospawn_support(&view(false, false, true, false)),
            Some("kitty")
        );
        assert_eq!(
            autospawn_support(&view(false, false, false, true)),
            Some("WezTerm")
        );
        assert_eq!(autospawn_support(&view(false, false, false, false)), None);
    }

    #[test]
    fn resolves_on_path_matches_the_running_binary() {
        // The test binary genuinely exists; a PATH containing its own dir must
        // resolve to it, and a PATH without it must not.
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap();
        let name = exe.file_name().unwrap();

        let has = std::env::join_paths([dir, Path::new("/no/such/dir")]).unwrap();
        assert!(resolves_on_path(&exe, Some(has.as_os_str()), name));

        let missing = std::env::join_paths([Path::new("/no/such/dir")]).unwrap();
        assert!(!resolves_on_path(&exe, Some(missing.as_os_str()), name));
        assert!(!resolves_on_path(&exe, None, name));
    }

    #[cfg(unix)]
    #[test]
    fn resolves_on_path_follows_a_symlinked_install() {
        use std::os::unix::fs::symlink;

        // Model a Homebrew-style layout: the real binary lives in a Cellar dir,
        // and the dir that's actually on PATH holds a symlink to it. A lexical
        // dir compare would miss this; resolving the link makes it match.
        let base = std::env::temp_dir().join(format!("termling-doctor-{}", std::process::id()));
        let cellar = base.join("Cellar/bin");
        let bindir = base.join("bin");
        std::fs::create_dir_all(&cellar).unwrap();
        std::fs::create_dir_all(&bindir).unwrap();

        let real = cellar.join("termling");
        std::fs::write(&real, b"#!/bin/sh\n").unwrap();
        symlink(&real, bindir.join("termling")).unwrap();

        let name = OsStr::new("termling");
        let on = std::env::join_paths([&bindir]).unwrap();
        assert!(resolves_on_path(&real, Some(on.as_os_str()), name));

        let off = std::env::join_paths([base.join("nope")]).unwrap();
        assert!(!resolves_on_path(&real, Some(off.as_os_str()), name));

        let _ = std::fs::remove_dir_all(&base);
    }
}
