//! The `init` subcommand: prints a shell hook you evaluate from your rc file.
//!
//! Install:
//!   zsh :  eval "$(termling init zsh)"        # ~/.zshrc
//!   bash:  eval "$(termling init bash)"        # ~/.bashrc
//!   fish:  termling init fish | source         # ~/.config/fish/config.fish
//!
//! With no shell argument, `termling init` guesses from `$SHELL` (your login
//! shell). If your interactive shell differs, name it: `termling init bash`.
//!
//! Every hook is purely additive — it registers the shell's command-lifecycle
//! callbacks and preserves any prompt hook you already had, so your shell,
//! prompt, and plugins keep working. Each callback shells out to `termling
//! emit`, which fires a fire-and-forget datagram and returns instantly whether
//! or not the sidebar is running. That call is also **backgrounded/disowned** so
//! the interactive prompt never waits on even the process spawn — the per-command
//! cost stays off the prompt's critical path. The socket/event machinery is
//! shell-agnostic; the only per-shell part is the hook text below.

use std::io;
use std::path::Path;

/// Entry point for `termling init [shell]`.
pub fn run(args: &[String]) -> io::Result<()> {
    // An explicit shell name always wins. With no argument we guess from
    // $SHELL; if that isn't one we support (sh/dash/unknown) we say so rather
    // than emit a zsh hook that's guaranteed to error in that shell.
    let shell: String = match args.first() {
        Some(s) => s.clone(),
        None => match detect_shell() {
            Some(s) => s.to_string(),
            None => {
                eprintln!(
                    "termling: couldn't tell your shell from $SHELL — run `termling init zsh`, `bash`, or `fish`."
                );
                return Ok(());
            }
        },
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
/// Returns `None` when `$SHELL` names a shell we have no hook for.
pub(crate) fn detect_shell() -> Option<&'static str> {
    shell_for(std::env::var("SHELL").ok().as_deref())
}

/// The pure mapping behind [`detect_shell`], split out so it can be tested.
/// Recognizes only shells we actually generate a hook for; `sh`, `dash`, and
/// anything unknown return `None` (a zsh hook would just error there).
fn shell_for(shell_path: Option<&str>) -> Option<&'static str> {
    let name = shell_path
        .and_then(|p| Path::new(p).file_name())
        .map(|f| f.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.contains("fish") {
        Some("fish")
    } else if name.contains("bash") {
        Some("bash")
    } else if name.contains("zsh") {
        Some("zsh")
    } else {
        None
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

# The emit is fire-and-forget, so it's backgrounded (`&!` = background + disown,
# no job-control chatter) — the prompt never waits on a process spawn.
__termling_preexec() {
  _termling_start=$EPOCHREALTIME
  "$_TERMLING_BIN" emit --type start --cmd "$1" --pwd "$PWD" --shell "$_termling_shell" &!
}

__termling_precmd() {
  local code=$?
  # Only report when a command actually ran (skip bare prompts / empty Enter).
  if (( _termling_start > 0 )); then
    local dur=$(( (EPOCHREALTIME - _termling_start) * 1000 ))
    "$_TERMLING_BIN" emit --type end --code $code --dur ${dur%.*} --pwd "$PWD" --shell "$_termling_shell" &!
    _termling_start=0
  fi
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __termling_preexec
add-zsh-hook precmd __termling_precmd
"#;

/// bash has no native preexec/precmd, so emulate them with a `DEBUG` trap
/// (start) and `PROMPT_COMMAND` (end + exit code). The trap fires before *every*
/// simple command — including tab-completion and each command inside
/// `PROMPT_COMMAND` — so a `_termling_ready` latch, raised only at the very tail
/// of `PROMPT_COMMAND` (after any prompt hook you already had) and cleared on the
/// first real command, keeps prompt machinery from being reported as your
/// command. The precmd runs *first* in `PROMPT_COMMAND` so it reads the true
/// exit code before anything else clobbers `$?`, and the trap always
/// `return 0`s so a failed command can't make `shopt -s extdebug` skip the next
/// one. Duration is whole-second (`$SECONDS`) so it works on macOS's stock bash
/// 3.2 (no `$EPOCHREALTIME`) and sidesteps locale decimal-separator issues.
///
/// bash allows only one `DEBUG` trap, so if you also use another preexec
/// framework (bash-preexec, starship's bash hook), load termling *last*. A
/// pipeline's `$PIPESTATUS` is reset by the trap between commands on a compound
/// line — a known limitation of every DEBUG-trap-based hook, not just this one.
const BASH_TEMPLATE: &str = r#"# termling — feed your shell's activity to the companion creature.
# Additive only: a DEBUG trap + PROMPT_COMMAND; changes nothing else.
_TERMLING_BIN='@TERMLING_BIN@'
_termling_shell="${TMUX_PANE:-$$}"
_termling_start=-1
_termling_ready=0

__termling_preexec() {
  # Ignore tab-completion and any fire before the prompt has armed us (the trap
  # also runs for PROMPT_COMMAND's own commands). Always return 0 so extdebug
  # never skips the command on a non-zero exit.
  if [[ -n ${COMP_LINE:-} ]] || (( _termling_ready == 0 )); then
    return 0
  fi
  _termling_ready=0
  _termling_start=$SECONDS
  # Fire-and-forget in a throwaway subshell so the prompt never waits on the
  # spawn and bash prints no job-control chatter.
  ( "$_TERMLING_BIN" emit --type start --cmd "$BASH_COMMAND" --pwd "$PWD" --shell "$_termling_shell" & ) 2>/dev/null
  return 0
}

__termling_precmd() {
  # First in PROMPT_COMMAND, so $? is still the command's own exit code.
  local code=$?
  if (( _termling_start >= 0 )); then
    ( "$_TERMLING_BIN" emit --type end --code "$code" --dur $(( (SECONDS - _termling_start) * 1000 )) --pwd "$PWD" --shell "$_termling_shell" & ) 2>/dev/null
    _termling_start=-1
  fi
  return $code
}

# Raised at the tail of PROMPT_COMMAND so the next DEBUG fire is a real command.
__termling_arm() { _termling_ready=1; }

# Auto-spawn the companion beside this shell; subshell keeps job control quiet.
if [[ -z ${TERMLING_COMPANION:-} && ${TERMLING_AUTOSPAWN:-1} != 0 ]]; then
  ( "$_TERMLING_BIN" ensure & ) 2>/dev/null
fi

# precmd first (reads $?), any existing hook next, arm last.
case ";${PROMPT_COMMAND:-};" in
  *";__termling_precmd;"*) ;;
  *) PROMPT_COMMAND="__termling_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND};__termling_arm" ;;
esac
# Install the DEBUG trap LAST so nothing sourced after it is caught at startup.
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
    # Fire-and-forget: background + disown so the prompt never waits on the spawn.
    "$_TERMLING_BIN" emit --type start --cmd "$argv" --pwd "$PWD" --shell "$_termling_shell" &
    disown
end

function __termling_postexec --on-event fish_postexec
    set -l code $status
    "$_TERMLING_BIN" emit --type end --code $code --dur $CMD_DURATION --pwd "$PWD" --shell "$_termling_shell" &
    disown
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
        // The emit is backgrounded (`&!`) so the prompt never waits on it.
        assert!(h.contains("--shell \"$_termling_shell\" &!"));
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
        // Both emits run in a throwaway background subshell so the prompt doesn't
        // wait on the spawn (and bash stays quiet about job control).
        assert!(h.contains("--shell \"$_termling_shell\" & ) 2>/dev/null"));
        // precmd reads the true exit code first thing, then restores it.
        assert!(h.contains("local code=$?"));
        assert!(h.contains("return $code"));
        // The "ready" latch is armed at the tail of PROMPT_COMMAND, so start
        // fires on the next real command and not on prompt machinery.
        assert!(h.contains("_termling_ready=0"));
        assert!(h.contains("__termling_arm"));
        // The DEBUG trap must be installed AFTER PROMPT_COMMAND is set, or the
        // assignment itself gets caught as a bogus startup command (finding #17).
        let trap_at = h.find("trap '__termling_preexec' DEBUG").unwrap();
        let pc_at = h.find("PROMPT_COMMAND=").unwrap();
        assert!(trap_at > pc_at, "DEBUG trap must be installed last");
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
        // Both emits are backgrounded + disowned so the prompt never waits.
        assert!(h.contains("--shell \"$_termling_shell\" &\n    disown"));
        assert!(!h.contains("@TERMLING_BIN@"));
    }

    #[test]
    fn detects_shell_from_shell_path() {
        assert_eq!(shell_for(Some("/usr/bin/fish")), Some("fish"));
        assert_eq!(shell_for(Some("/bin/bash")), Some("bash"));
        assert_eq!(shell_for(Some("/usr/local/bin/bash")), Some("bash"));
        assert_eq!(shell_for(Some("/bin/zsh")), Some("zsh"));
        // sh, dash, or unknown: no hook we can trust, so no guess.
        assert_eq!(shell_for(Some("/bin/sh")), None);
        assert_eq!(shell_for(Some("/usr/bin/dash")), None);
        assert_eq!(shell_for(None), None);
    }
}
