//! The `emit` subcommand: the shell hook calls this once per command to fire an
//! [`Event`](crate::event::Event) at the sidebar.
//!
//! It must be cheap and it must never block — a datagram send returns in
//! microseconds even when nothing is listening (see [`crate::event::send`]).
//!
//! Usage (from the generated hook):
//!   termling emit --type start --cmd "<cmdline>"
//!   termling emit --type end --code <n> [--dur <ms>] [--pwd <dir>] [--cmd "<cmdline>"]

use std::io;

use crate::event::{Event, Kind};

/// How much of the command line is allowed to leave the shell. Commands can
/// carry secrets (`export TOKEN=…`), so the default is conservative.
enum Privacy {
    /// Send the whole command line.
    Full,
    /// Send only the command's first word (the program name). The default.
    Name,
    /// Send no command text at all.
    None,
}

impl Privacy {
    fn from_env() -> Privacy {
        match std::env::var("TERMLING_PRIVACY").as_deref() {
            Ok("full") => Privacy::Full,
            Ok("none") => Privacy::None,
            _ => Privacy::Name,
        }
    }

    /// Apply the policy to a raw command line.
    fn apply(&self, cmd: Option<String>) -> Option<String> {
        let cmd = cmd?;
        match self {
            Privacy::Full => Some(cmd),
            Privacy::None => None,
            // First whitespace-delimited word, ignoring a leading `VAR=… ` if any.
            Privacy::Name => cmd
                .split_whitespace()
                .find(|w| !w.contains('='))
                .map(str::to_string),
        }
    }
}

/// Entry point for `termling emit …`. Always returns `Ok(())` — a failed
/// send is not the shell's problem.
pub fn run(args: &[String]) -> io::Result<()> {
    let ev = build_event(args, &Privacy::from_env());
    crate::event::send(&ev);
    Ok(())
}

/// Parse the `emit` flags into an [`Event`], applying `privacy` to the command
/// text. Split out from [`run`] (which also sends) so the flag handling — the
/// part with edge cases — is unit-testable without touching the socket.
fn build_event(args: &[String], privacy: &Privacy) -> Event {
    let mut kind = Kind::End;
    let mut code: Option<i32> = None;
    let mut dur_ms: Option<u64> = None;
    let mut pwd: Option<String> = None;
    let mut shell: Option<String> = None;
    let mut cmd: Option<String> = None;

    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--type" => {
                kind = match it.next().map(String::as_str) {
                    Some("start") => Kind::Start,
                    _ => Kind::End,
                }
            }
            "--code" => code = it.next().and_then(|v| v.parse().ok()),
            "--dur" => dur_ms = it.next().and_then(|v| v.parse().ok()),
            "--pwd" => pwd = it.next().cloned(),
            // An opaque shell id (tmux pane / tty / pid) — not command text, so
            // it bypasses the privacy filter below.
            "--shell" => shell = it.next().cloned(),
            "--cmd" => cmd = it.next().cloned(),
            _ => {}
        }
    }

    let cmd = privacy.apply(cmd);
    match kind {
        Kind::Start => {
            // A `start` carries where and which shell it came from, so the
            // companion can slot it into the right roster entry immediately.
            let mut ev = Event::start(cmd);
            ev.pwd = pwd;
            ev.shell = shell;
            ev
        }
        Kind::End => {
            // An `end` always carries a code (default 0), plus whatever context
            // the hook measured.
            let mut ev = Event::end(code.unwrap_or(0));
            ev.dur_ms = dur_ms;
            ev.pwd = pwd;
            ev.shell = shell;
            ev.cmd = cmd;
            ev
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_policy_keeps_only_the_program() {
        assert_eq!(
            Privacy::Name.apply(Some("git push origin main".into())),
            Some("git".into())
        );
    }

    #[test]
    fn name_policy_skips_leading_env_assignments() {
        assert_eq!(
            Privacy::Name.apply(Some("FOO=bar TOKEN=sekret cargo test".into())),
            Some("cargo".into())
        );
    }

    #[test]
    fn none_policy_drops_the_command() {
        assert_eq!(Privacy::None.apply(Some("anything".into())), None);
    }

    #[test]
    fn full_policy_keeps_everything() {
        assert_eq!(
            Privacy::Full.apply(Some("echo hi there".into())),
            Some("echo hi there".into())
        );
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn build_event_parses_a_start() {
        let ev = build_event(
            &args(&[
                "--type", "start", "--cmd", "git push", "--pwd", "/tmp", "--shell", "%1",
            ]),
            &Privacy::Full,
        );
        assert_eq!(ev.kind, Kind::Start);
        assert_eq!(ev.cmd.as_deref(), Some("git push"));
        assert_eq!(ev.pwd.as_deref(), Some("/tmp"));
        assert_eq!(ev.shell.as_deref(), Some("%1"));
        // A start has no exit code.
        assert_eq!(ev.code, None);
    }

    #[test]
    fn build_event_defaults_end_code_to_zero() {
        // No --type ⇒ End; no --code ⇒ 0 (an `end` always carries a code).
        let ev = build_event(&args(&["--dur", "1500", "--pwd", "/x"]), &Privacy::Name);
        assert_eq!(ev.kind, Kind::End);
        assert_eq!(ev.code, Some(0));
        assert_eq!(ev.dur_ms, Some(1500));
    }

    #[test]
    fn build_event_applies_privacy_to_the_command() {
        let ev = build_event(
            &args(&["--type", "start", "--cmd", "psql -U admin topsecret"]),
            &Privacy::Name,
        );
        // Only the program name leaves the shell under the default policy.
        assert_eq!(ev.cmd.as_deref(), Some("psql"));
    }

    #[test]
    fn build_event_ignores_unparseable_numbers() {
        let ev = build_event(
            &args(&["--type", "end", "--code", "NaN", "--dur", "soon"]),
            &Privacy::Name,
        );
        // A junk --code falls back to the 0 default; a junk --dur stays absent.
        assert_eq!(ev.code, Some(0));
        assert_eq!(ev.dur_ms, None);
    }

    #[test]
    fn event_round_trips_a_multiline_command() {
        // Shells can produce a newline inside a command; the wire format must
        // carry it intact (only a literal \x1f — which shells never emit — would
        // confuse the field split).
        let ev = build_event(
            &args(&["--type", "start", "--cmd", "echo one\ntwo"]),
            &Privacy::Full,
        );
        let decoded = crate::event::Event::decode(&ev.encode()).unwrap();
        assert_eq!(decoded, ev);
        assert_eq!(decoded.cmd.as_deref(), Some("echo one\ntwo"));
    }
}
