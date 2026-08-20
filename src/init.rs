//! The `init` subcommand: prints a shell hook you evaluate from your rc file.
//!
//! Install:
//!   zsh :  eval "$(termling init zsh)"        # ~/.zshrc
//!   bash:  eval "$(termling init bash)"        # ~/.bashrc
//!   fish:  termling init fish | source         # ~/.config/fish/config.fish
//!
//! With no shell argument, `termling init` guesses from `$SHELL`.
//!
//! Every hook is purely additive — it registers the shell's command-lifecycle
//! callbacks and touches nothing else, so your shell, prompt, and plugins are
//! unchanged. Each callback shells out to `termling emit`, which fires a
//! fire-and-forget datagram and returns instantly whether or not the sidebar is
//! running. The socket/event machinery is shell-agnostic; the only per-shell
//! part is the hook text below.

use std::io;
use std::path::Path;

/// Entry point for `termling init [shell]`.
pub fn run(args: &[String]) -> io::Result<()> {
    let shell = match args.first() {
        Some(s) => s.clone(),
        None => detect_shell().to_string(),
    };
    match shell.as_str() {
        "zsh" => print!("{}", zsh_hook()),
        "bash" => print!("{}", bash_hook()),
        "fish" => print!("{}", fish_hook()),
        other => {
            eprintln!("termling: init supports zsh, bash, fish (got '{other}')");
        }
    }
    Ok(())
}

/// The absolute path to this binary, so a hook works even when it isn't on
/// `PATH`. Falls back to the bare name if we can't resolve it.
fn bin_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "termling".to_string())
}

/// Fill the `@TERMLING_BIN@` placeholder in a hook template with the resolved
/// binary path. Using a placeholder (rather than `format!`) keeps the shell
/// scripts readable — no doubling of their many literal `{`/`}`.
fn render(template: &str) -> String {
    template.replace("@TERMLING_BIN@", &bin_path())
}

/// Best-effort guess of the current shell from `$SHELL` (its basename). This is
/// the login shell, which matches the interactive shell for almost everyone; a
/// shell named explicitly on the command line always wins. Shell-identifying
/// vars like `$ZSH_VERSION` aren't exported to us, so `$SHELL` is the signal.
pub(crate) fn detect_shell() -> &'static str {
    shell_for(std::env::var("SHELL").ok().as_deref())
}

/// The pure mapping behind [`detect_shell`], split out so it can be tested.
fn shell_for(shell_path: Option<&str>) -> &'static str {
    let name = shell_path
        .and_then(|p| Path::new(p).file_name())
        .map(|f| f.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.contains("fish") {
        "fish"
    } else if name.contains("bash") {
        "bash"
    } else {
        // zsh, sh, dash, or unknown — default to the zsh hook.
        "zsh"
    }
}

fn zsh_hook() -> String {
    render(ZSH_TEMPLATE)
}

fn bash_hook() -> String {
    render(BASH_TEMPLATE)
}

fn fish_hook() -> String {
    render(FISH_TEMPLATE)
}

/// zsh: native `preexec`/`precmd` via `add-zsh-hook`, sub-ms duration from
/// `$EPOCHREALTIME`.
const ZSH_TEMPLATE: &str = r#"# termling — feed your shell's activity to the companion creature.
# Additive only: registers preexec/precmd hooks; changes nothing else.
zmodload zsh/datetime 2>/dev/null
_TERMLING_BIN='@TERMLING_BIN@'
typeset -g _termling_start=0
# A stable id for this shell so the companion can track several at once:
# tmux pane if we're in tmux, else the tty name, else the pid.
typeset -g _termling_shell="${TMUX_PANE:-${TTY:t}}"
[[ -z $_termling_shell ]] && _termling_shell=$$

# Auto-spawn the companion beside this shell (tmux split / iTerm AppleScript).
# Skipped inside the companion's own pane and when TERMLING_AUTOSPAWN=0.
# Backgrounded inside a throwaway subshell so startup never waits on it and zsh
# reports no job-control chatter ([N] pid … done) for the spawn.
if [[ -z $TERMLING_COMPANION && ${TERMLING_AUTOSPAWN:-1} != 0 ]]; then
  ( "$_TERMLING_BIN" ensure & ) 2>/dev/null
fi

__termling_preexec() {
  _termling_start=$EPOCHREALTIME
  "$_TERMLING_BIN" emit --type start --cmd "$1" --pwd "$PWD" --shell "$_termling_shell"
}

__termling_precmd() {
  local code=$?
  # Only report when a command actually ran (skip bare prompts / empty Enter).
  if (( _termling_start > 0 )); then
    local dur=$(( (EPOCHREALTIME - _termling_start) * 1000 ))
    "$_TERMLING_BIN" emit --type end --code $code --dur ${dur%.*} --pwd "$PWD" --shell "$_termling_shell"
    _termling_start=0
  fi
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __termling_preexec
add-zsh-hook precmd __termling_precmd
"#;

/// bash: no native preexec/precmd, so emulate them with a `DEBUG` trap (start)
/// and a `PROMPT_COMMAND`-prepended precmd (end + exit code). A single "armed"
/// flag makes start fire once per command line, not once per simple command.
/// The DEBUG trap captures `$?` first and restores it with `return $__ec`, so
/// the exit code the precmd reads is the *user's*, not the trap's. Duration is
/// whole-second (`$SECONDS`) for portability — works on macOS's stock bash 3.2,
/// where `$EPOCHREALTIME` doesn't exist, and avoids locale decimal-separator
/// pitfalls. Note: this installs a DEBUG trap; another preexec framework in the
/// same shell may conflict.
const BASH_TEMPLATE: &str = r#"# termling — feed your shell's activity to the companion creature.
# Additive only: a DEBUG trap + PROMPT_COMMAND; changes nothing else.
_TERMLING_BIN='@TERMLING_BIN@'
_termling_shell="${TMUX_PANE:-$$}"
_termling_start=-1
_termling_armed=1

__termling_preexec() {
  local __ec=$?
  # Skip tab-completion and re-entrant fires within the same command line.
  { [[ -n ${COMP_LINE:-} ]] || (( _termling_armed == 0 )); } && return $__ec
  local cmd=$BASH_COMMAND
  [[ $cmd == __termling_precmd* ]] && return $__ec
  _termling_armed=0
  _termling_start=$SECONDS
  "$_TERMLING_BIN" emit --type start --cmd "$cmd" --pwd "$PWD" --shell "$_termling_shell"
  return $__ec
}

__termling_precmd() {
  local code=$?
  if (( _termling_start >= 0 )); then
    "$_TERMLING_BIN" emit --type end --code "$code" --dur $(( (SECONDS - _termling_start) * 1000 )) --pwd "$PWD" --shell "$_termling_shell"
    _termling_start=-1
  fi
  _termling_armed=1
}

# Auto-spawn the companion beside this shell; subshell keeps job control quiet.
if [[ -z ${TERMLING_COMPANION:-} && ${TERMLING_AUTOSPAWN:-1} != 0 ]]; then
  ( "$_TERMLING_BIN" ensure & ) 2>/dev/null
fi

case ";${PROMPT_COMMAND:-};" in
  *";__termling_precmd;"*) ;;
  *) PROMPT_COMMAND="__termling_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac
# Install the DEBUG trap LAST: anything sourced after it (e.g. the case above)
# would otherwise be caught as a bogus "command" and reported once at startup.
trap '__termling_preexec' DEBUG
"#;

/// fish: native `fish_preexec`/`fish_postexec` events and the built-in
/// `$CMD_DURATION` (already milliseconds). `$status` is captured first so the
/// emit call can't clobber it.
const FISH_TEMPLATE: &str = r#"# termling — feed your shell's activity to the companion creature.
# Additive only: registers fish_preexec/fish_postexec events; nothing else.
set -g _TERMLING_BIN '@TERMLING_BIN@'
if set -q TMUX_PANE
    set -g _termling_shell $TMUX_PANE
else
    set -g _termling_shell $fish_pid
end

function __termling_preexec --on-event fish_preexec
    "$_TERMLING_BIN" emit --type start --cmd "$argv" --pwd "$PWD" --shell "$_termling_shell"
end

function __termling_postexec --on-event fish_postexec
    set -l code $status
    "$_TERMLING_BIN" emit --type end --code $code --dur $CMD_DURATION --pwd "$PWD" --shell "$_termling_shell"
end

# Auto-spawn the companion beside this shell (disown so it isn't tracked).
if not set -q TERMLING_COMPANION; and test "$TERMLING_AUTOSPAWN" != 0
    "$_TERMLING_BIN" ensure &
    disown
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_hook_wires_the_native_callbacks() {
        let h = zsh_hook();
        assert!(h.contains("add-zsh-hook preexec __termling_preexec"));
        assert!(h.contains("add-zsh-hook precmd __termling_precmd"));
        assert!(h.contains("emit --type start"));
        assert!(h.contains("emit --type end"));
        // The binary path was substituted (no leftover placeholder).
        assert!(!h.contains("@TERMLING_BIN@"));
    }

    #[test]
    fn bash_hook_uses_debug_trap_and_prompt_command() {
        let h = bash_hook();
        assert!(h.contains("trap '__termling_preexec' DEBUG"));
        assert!(h.contains("PROMPT_COMMAND="));
        assert!(h.contains("emit --type start"));
        assert!(h.contains("emit --type end"));
        // Exit code is preserved across the DEBUG trap.
        assert!(h.contains("local __ec=$?"));
        assert!(h.contains("return $__ec"));
        assert!(!h.contains("@TERMLING_BIN@"));
    }

    #[test]
    fn fish_hook_uses_native_events_and_cmd_duration() {
        let h = fish_hook();
        assert!(h.contains("--on-event fish_preexec"));
        assert!(h.contains("--on-event fish_postexec"));
        assert!(h.contains("$CMD_DURATION"));
        // Exit status captured before the emit call can clobber it.
        assert!(h.contains("set -l code $status"));
        assert!(!h.contains("@TERMLING_BIN@"));
    }

    #[test]
    fn detects_shell_from_shell_path() {
        assert_eq!(shell_for(Some("/usr/bin/fish")), "fish");
        assert_eq!(shell_for(Some("/bin/bash")), "bash");
        assert_eq!(shell_for(Some("/usr/local/bin/bash")), "bash");
        assert_eq!(shell_for(Some("/bin/zsh")), "zsh");
        // Unknown or POSIX sh falls back to the zsh hook.
        assert_eq!(shell_for(Some("/bin/sh")), "zsh");
        assert_eq!(shell_for(None), "zsh");
    }
}
