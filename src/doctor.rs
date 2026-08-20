//! The `doctor` subcommand: a quick, network-free environment check that
//! answers "will the companion work here, and if it didn't spawn, why?". It
//! reuses the same scope/socket/liveness logic the real companion uses, so what
//! it reports is exactly what `ensure`/`tmux` see.

use std::io;

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

/// Is the running binary's directory on `$PATH` (so the hook's bare `termling`
/// resolves)?
fn on_path() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent() else {
        return false;
    };
    match std::env::var_os("PATH") {
        Some(path) => std::env::split_paths(&path).any(|p| p == dir),
        None => false,
    }
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
        "fish" => "termling init fish | source",
        "bash" => "eval \"$(termling init bash)\"",
        _ => "eval \"$(termling init zsh)\"",
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
}
