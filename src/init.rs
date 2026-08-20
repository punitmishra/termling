//! The `init` subcommand: prints a shell hook you `eval` from your rc file.
//!
//! Install (zsh):
//!   eval "$(termling init zsh)"
//!
//! The hook is purely additive — it registers `preexec`/`precmd` callbacks and
//! touches nothing else, so your shell, prompt, and plugins are unchanged. Each
//! callback shells out to `termling emit`, which fires a fire-and-forget
//! datagram and returns instantly whether or not the sidebar is running.

use std::io;

/// Entry point for `termling init <shell>`.
pub fn run(args: &[String]) -> io::Result<()> {
    match args.first().map(String::as_str) {
        Some("zsh") | None => {
            print!("{}", zsh_hook());
            Ok(())
        }
        Some(other) => {
            eprintln!("termling: init supports 'zsh' (got '{other}')");
            Ok(())
        }
    }
}

/// The absolute path to this binary, so the hook works even when it isn't on
/// `PATH`. Falls back to the bare name if we can't resolve it.
fn bin_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "termling".to_string())
}

fn zsh_hook() -> String {
    let bin = bin_path();
    format!(
        r#"# termling — feed your shell's activity to the companion creature.
# Additive only: registers preexec/precmd hooks; changes nothing else.
zmodload zsh/datetime 2>/dev/null
_TERMLING_BIN='{bin}'
typeset -g _termling_start=0
# A stable id for this shell so the companion can track several at once:
# tmux pane if we're in tmux, else the tty name, else the pid.
typeset -g _termling_shell="${{TMUX_PANE:-${{TTY:t}}}}"
[[ -z $_termling_shell ]] && _termling_shell=$$

# Auto-spawn the companion beside this shell (tmux split / iTerm AppleScript).
# Skipped inside the companion's own pane and when TERMLING_AUTOSPAWN=0.
# Backgrounded so shell startup never waits on it.
if [[ -z $TERMLING_COMPANION && ${{TERMLING_AUTOSPAWN:-1}} != 0 ]]; then
  {{ "$_TERMLING_BIN" ensure & }} 2>/dev/null
fi

__termling_preexec() {{
  _termling_start=$EPOCHREALTIME
  "$_TERMLING_BIN" emit --type start --cmd "$1" --pwd "$PWD" --shell "$_termling_shell"
}}

__termling_precmd() {{
  local code=$?
  # Only report when a command actually ran (skip bare prompts / empty Enter).
  if (( _termling_start > 0 )); then
    local dur=$(( (EPOCHREALTIME - _termling_start) * 1000 ))
    "$_TERMLING_BIN" emit --type end --code $code --dur ${{dur%.*}} --pwd "$PWD" --shell "$_termling_shell"
    _termling_start=0
  fi
}}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __termling_preexec
add-zsh-hook precmd __termling_precmd
"#
    )
}
