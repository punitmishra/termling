//! Opening the companion in a split beside your shell — both on demand (the
//! `tmux` subcommand) and automatically (the `ensure` subcommand the shell hook
//! fires at startup).
//!
//! Four window managers can split themselves: **tmux** (native `split-window`),
//! **iTerm2** (via AppleScript), **kitty** (`kitten @ launch`, needs remote
//! control), and **WezTerm** (`wezterm cli split-pane`). Everywhere else we just
//! print the recipe. A companion spawned here inherits `TERMLING_COMPANION=1`
//! (so its own shell never spawns another) and the resolved `TERMLING_SCOPE` (so
//! it binds the exact socket the shell emits to).

use std::io;
use std::process::Command;

use crate::event;

/// Width of the companion pane, in columns.
const SIDEBAR_COLS: &str = "34";

/// The `tmux` subcommand: open the companion beside your shell now, if we can.
pub fn launch() -> io::Result<()> {
    let scope = event::scope();
    if event::is_live(&scope) {
        eprintln!("termling: a companion is already running here (scope {scope}).");
        return Ok(());
    }
    if !spawn_companion(&scope) {
        print_recipe(&exe_path());
    }
    Ok(())
}

/// The `ensure` subcommand: the hook fires this (backgrounded) when a shell
/// starts. Spawn a companion only if this scope doesn't already have one — and
/// never from inside the companion's own shell, which would recurse.
pub fn ensure() -> io::Result<()> {
    if std::env::var_os("TERMLING_COMPANION").is_some() {
        return Ok(());
    }
    if matches!(std::env::var("TERMLING_AUTOSPAWN").as_deref(), Ok("0")) {
        return Ok(());
    }
    let scope = event::scope();
    if !event::is_live(&scope) {
        spawn_companion(&scope);
    }
    Ok(())
}

/// Try to split off a companion pane for `scope`. Returns whether a split was
/// actually launched (false ⇒ no supported window manager, caller may fall back
/// to printing the recipe).
fn spawn_companion(scope: &str) -> bool {
    let exe = exe_path();

    if std::env::var_os("TMUX").is_some() {
        // Split to the right, keep focus on the shell (-d); pass the guard env
        // and scope into the new pane so it binds the right socket.
        let scope_env = format!("TERMLING_SCOPE={scope}");
        let status = Command::new("tmux")
            .args([
                "split-window",
                "-h",
                "-d",
                "-l",
                SIDEBAR_COLS,
                "-e",
                "TERMLING_COMPANION=1",
                "-e",
                &scope_env,
                &exe,
            ])
            .status();
        return matches!(status, Ok(s) if s.success());
    }

    if std::env::var_os("ITERM_SESSION_ID").is_some() {
        // iTerm2 has no CLI for splitting, so drive it with AppleScript. The new
        // session runs `env … <exe>` so it carries the guard + scope.
        let inner = format!("env TERMLING_COMPANION=1 TERMLING_SCOPE={scope} '{exe}'");
        let script = format!(
            "tell application \"iTerm2\" to tell current session of current window \
             to split vertically with default profile command \"{}\"",
            applescript_escape(&inner)
        );
        let status = Command::new("osascript").args(["-e", &script]).status();
        return matches!(status, Ok(s) if s.success());
    }

    if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        // kitty's remote control: split a new OS-window-less pane to the side and
        // keep focus on the shell. Requires `allow_remote_control` in kitty.conf;
        // if it's off the command fails and we fall through to the recipe.
        let status = Command::new("kitten")
            .args([
                "@",
                "launch",
                "--type=window",
                "--location=vsplit",
                "--dont-take-focus",
                "--",
                "env",
                "TERMLING_COMPANION=1",
                &format!("TERMLING_SCOPE={scope}"),
                &exe,
            ])
            .status();
        return matches!(status, Ok(s) if s.success());
    }

    if std::env::var_os("WEZTERM_PANE").is_some() {
        // WezTerm's CLI splits the active pane; it moves focus to the new pane, so
        // hand focus back to the shell afterward. Best-effort — a failed activate
        // still leaves a working companion.
        let status = Command::new("wezterm")
            .args([
                "cli",
                "split-pane",
                "--right",
                "--",
                "env",
                "TERMLING_COMPANION=1",
                &format!("TERMLING_SCOPE={scope}"),
                &exe,
            ])
            .status();
        if matches!(status, Ok(s) if s.success()) {
            let shell_pane = std::env::var("WEZTERM_PANE").unwrap_or_default();
            let _ = Command::new("wezterm")
                .args(["cli", "activate-pane", "--pane-id", &shell_pane])
                .status();
            return true;
        }
        return false;
    }

    false
}

/// The absolute path to this binary, so a spawned pane finds it even off `PATH`.
fn exe_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "termling".to_string())
}

/// Escape a string for embedding inside an AppleScript double-quoted literal.
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn print_recipe(exe: &str) {
    println!("termling — put the creature beside your shell:\n");
    println!("  tmux    : run this from inside tmux and it splits automatically,");
    println!("            or manually:  tmux split-window -h -l {SIDEBAR_COLS} '{exe}'");
    println!("  iTerm2  : runs automatically once the hook is installed;");
    println!("            it also splits when you run this from an iTerm window.");
    println!("  kitty   : auto-splits with remote control on");
    println!("            (put `allow_remote_control yes` in kitty.conf).");
    println!("  WezTerm : auto-splits via `wezterm cli` once the hook is installed.");
    println!("  zellij  : zellij --layout contrib/termling.kdl");
    println!("  plain   : open a second terminal window and run:  {exe}");
    println!("\nAnd add the hook to your shell rc so the creature reacts (and auto-spawns):");
    println!("  zsh :  eval \"$({exe} init zsh)\"       # ~/.zshrc");
    println!("  bash:  eval \"$({exe} init bash)\"      # ~/.bashrc");
    println!("  fish:  {exe} init fish | source        # ~/.config/fish/config.fish");
}
