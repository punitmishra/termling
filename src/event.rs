//! The event stream between your real shell and the sidebar.
//!
//! A [`shell hook`](crate::init) fires one [`Event`] per command over a Unix
//! *datagram* socket; the sidebar binds that socket and drains it each frame.
//! Datagrams are fire-and-forget: if the sidebar isn't running, the send fails
//! instantly and the shell never blocks.
//!
//! The wire format is a `\x1f`-delimited list of `key=value` fields — no JSON,
//! no serde, trivial to emit from zsh and to parse here. One datagram is exactly
//! one event, so there's no framing to worry about and the command text needs no
//! escaping (a lone `\x1f` inside a command is the only thing that could confuse
//! the split, and shells never produce one).

use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;

/// The unit-separator that delimits fields on the wire.
const SEP: char = '\x1f';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A command just started running.
    Start,
    /// A command finished, carrying its exit code.
    End,
}

/// One thing that happened in the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub code: Option<i32>,
    pub dur_ms: Option<u64>,
    pub pwd: Option<String>,
    /// An opaque id for the shell that fired this (tmux pane / tty / pid), so the
    /// companion can keep a roster of several shells at once. Not command text,
    /// so it's exempt from the privacy knob.
    pub shell: Option<String>,
    pub cmd: Option<String>,
}

impl Event {
    pub fn start(cmd: Option<String>) -> Event {
        Event {
            kind: Kind::Start,
            code: None,
            dur_ms: None,
            pwd: None,
            shell: None,
            cmd,
        }
    }

    pub fn end(code: i32) -> Event {
        Event {
            kind: Kind::End,
            code: Some(code),
            dur_ms: None,
            pwd: None,
            shell: None,
            cmd: None,
        }
    }

    /// Serialize to the wire form. `cmd` is placed last.
    pub fn encode(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!(
            "t={}",
            match self.kind {
                Kind::Start => "start",
                Kind::End => "end",
            }
        ));
        if let Some(c) = self.code {
            parts.push(format!("code={c}"));
        }
        if let Some(d) = self.dur_ms {
            parts.push(format!("dur={d}"));
        }
        if let Some(p) = &self.pwd {
            parts.push(format!("pwd={p}"));
        }
        if let Some(sh) = &self.shell {
            parts.push(format!("sh={sh}"));
        }
        if let Some(cmd) = &self.cmd {
            parts.push(format!("cmd={cmd}"));
        }
        parts.join(&SEP.to_string())
    }

    /// Parse the wire form. Unknown keys are ignored; a missing/!start,!end type
    /// yields `None`.
    pub fn decode(s: &str) -> Option<Event> {
        let mut kind: Option<Kind> = None;
        let mut code = None;
        let mut dur_ms = None;
        let mut pwd = None;
        let mut shell = None;
        let mut cmd = None;
        for field in s.split(SEP) {
            let (k, v) = field.split_once('=')?;
            match k {
                "t" => {
                    kind = match v {
                        "start" => Some(Kind::Start),
                        "end" => Some(Kind::End),
                        _ => return None,
                    }
                }
                "code" => code = v.parse().ok(),
                "dur" => dur_ms = v.parse().ok(),
                "pwd" => pwd = Some(v.to_string()),
                "sh" => shell = Some(v.to_string()),
                "cmd" => cmd = Some(v.to_string()),
                _ => {}
            }
        }
        Some(Event {
            kind: kind?,
            code,
            dur_ms,
            pwd,
            shell,
            cmd,
        })
    }
}

/// A *scope* is one companion's territory: every shell in the same scope shares
/// a socket and a single companion, which tracks them as a roster. This resolves
/// identically in the emitting shell and in the companion — they must agree, or
/// events would miss. Precedence:
///
///   1. `TERMLING_SCOPE` — an explicit override (and what we pass to a
///      companion we spawn, so it inherits the exact scope).
///   2. `$TMUX` present → its session id (the third comma field) → one companion
///      per tmux *session*, however many panes it has.
///   3. `$ITERM_SESSION_ID` (`wN tM pK:UUID`) → its `wN` window token → one per
///      iTerm window.
///   4. `"default"`.
pub fn scope() -> String {
    if let Some(s) = env_string("TERMLING_SCOPE") {
        let s = sanitize(&s);
        if !s.is_empty() {
            return s;
        }
    }
    if let Some(tmux) = env_string("TMUX") {
        // "$TMUX" is "<socket path>,<server pid>,<session id>".
        if let Some(id) = tmux.rsplit(',').next() {
            let id = sanitize(id);
            if !id.is_empty() {
                return format!("tmux-{id}");
            }
        }
    }
    if let Some(iterm) = env_string("ITERM_SESSION_ID") {
        // "wN tM pK:UUID" — the window token is the leading `wN`.
        let win: String = iterm
            .chars()
            .take_while(|c| *c != 't' && *c != ':')
            .collect();
        let win = sanitize(&win);
        if !win.is_empty() {
            return format!("iterm-{win}");
        }
    }
    "default".to_string()
}

fn env_string(key: &str) -> Option<String> {
    std::env::var_os(key).and_then(|v| v.into_string().ok())
}

/// Keep only filename-safe characters (and cap the length) so a scope is always
/// a valid, bounded socket-file name.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect()
}

/// Where the datagram socket lives — a per-user runtime dir if the platform
/// provides one, else the (already per-user) temp dir — one file per scope.
pub fn socket_path(scope: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("termling-{scope}.sock"))
}

/// Is a companion already bound and draining this scope's socket? Probes by
/// sending an empty datagram: it lands only if someone is bound (the companion
/// ignores the empty payload), and fails with `ConnectionRefused` for a stale
/// socket file left by a crashed run. Dependency-free single-instance check.
pub fn is_live(scope: &str) -> bool {
    let path = socket_path(scope);
    if !path.exists() {
        return false;
    }
    match UnixDatagram::unbound() {
        Ok(sock) => sock.send_to(&[], &path).is_ok(),
        Err(_) => false,
    }
}

/// Fire one event at the companion for this shell's scope. Never blocks and never
/// errors upward: if nobody is listening the send simply fails and we shrug.
pub fn send(ev: &Event) {
    if let Ok(sock) = UnixDatagram::unbound() {
        let _ = sock.send_to(ev.encode().as_bytes(), socket_path(&scope()));
    }
}

/// The companion's end: owns the bound socket and hands over whatever has
/// arrived. Bound to a single scope's socket for its lifetime.
pub struct Receiver {
    sock: UnixDatagram,
    buf: Vec<u8>,
    path: PathBuf,
}

impl Receiver {
    /// Bind this scope's socket in non-blocking mode so draining never stalls the
    /// render loop.
    ///
    /// The bind doubles as a lock that closes the auto-spawn race: creating the
    /// socket file is atomic, so if two companions start at once only one bind
    /// succeeds. The loser (`AddrInUse`) probes — if a live companion already
    /// owns the scope it returns `AddrInUse` for the caller to exit quietly;
    /// otherwise the file is a stale leftover, which it clears and rebinds.
    pub fn bind() -> io::Result<Receiver> {
        let path = socket_path(&scope());
        let sock = match UnixDatagram::bind(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                if is_live(&scope()) {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "a companion is already running for this scope",
                    ));
                }
                // A leftover socket from a crashed run — clear it and rebind.
                let _ = std::fs::remove_file(&path);
                UnixDatagram::bind(&path)?
            }
            Err(e) => return Err(e),
        };
        sock.set_nonblocking(true)?;
        Ok(Receiver {
            sock,
            buf: vec![0u8; 8192],
            path,
        })
    }

    /// Every event waiting in the socket buffer, decoded. Returns empty when the
    /// socket is quiet.
    pub fn drain(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            match self.sock.recv(&mut self.buf) {
                Ok(n) => {
                    if let Ok(text) = std::str::from_utf8(&self.buf[..n]) {
                        if let Some(ev) = Event::decode(text) {
                            out.push(ev);
                        }
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        out
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        // Tidy up our own scope's socket file so the next run binds cleanly.
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_round_trips() {
        let ev = Event::start(Some("cargo test".into()));
        let back = Event::decode(&ev.encode()).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn end_round_trips_with_all_fields() {
        let ev = Event {
            kind: Kind::End,
            code: Some(1),
            dur_ms: Some(1234),
            pwd: Some("/home/me/proj".into()),
            shell: Some("%3".into()),
            cmd: Some("false".into()),
        };
        assert_eq!(Event::decode(&ev.encode()).unwrap(), ev);
    }

    #[test]
    fn the_shell_id_round_trips() {
        let mut ev = Event::start(Some("cargo build".into()));
        ev.shell = Some("ttys004".into());
        ev.pwd = Some("/home/me/proj".into());
        assert_eq!(Event::decode(&ev.encode()).unwrap(), ev);
    }

    #[test]
    fn sanitize_keeps_only_filename_safe_chars() {
        assert_eq!(sanitize("w0"), "w0");
        assert_eq!(sanitize("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize("a b/c:d"), "abcd");
    }

    #[test]
    fn a_command_with_spaces_and_equals_survives() {
        // '=' inside the value must not confuse the split (cmd is last, and we
        // split each field only on the first '=').
        let ev = Event::start(Some("FOO=bar echo hi=there".into()));
        assert_eq!(Event::decode(&ev.encode()).unwrap(), ev);
    }

    #[test]
    fn garbage_decodes_to_none() {
        assert!(Event::decode("not-a-field").is_none());
        assert!(Event::decode("t=bogus").is_none());
    }
}
