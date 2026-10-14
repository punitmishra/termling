//! Application state and the logic that mutates it.
//!
//! termling is a *companion* to your real shell: your shell stays exactly
//! as it is (its own terminal, its own config, completions, and prompt), and
//! this process renders the mood creature and the arcade in a neighbouring pane,
//! reacting to events your shell fires over a socket (see [`crate::event`]).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::buddy::{Buddy, Quip};
use crate::doodle::Character;
use crate::event::{Event, Kind};
use crate::games::{GameKind, Input, MiniGame};
use crate::mood::{Mood, Signals};
use crate::theme::{self, Theme};

/// What the companion is showing: the ambient creature, the arcade picker, or a
/// game in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// At rest: the creature reacts to your real shell. No text input — your
    /// shell handles that.
    Ambient,
    /// The arcade picker.
    Menu,
    /// A game in progress.
    Game,
}

/// A brief celebratory flourish overlaid on the ambient creature, then it fades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flair {
    /// Crossed into a new level.
    LevelUp,
    /// A success streak hit a round milestone.
    Milestone,
    /// A long-running command just finished.
    LongDone,
}

/// What the companion knows about one shell in its scope. Several shells (tmux
/// panes, iTerm tabs) share one companion; each gets a roster entry.
pub struct ShellState {
    /// The shell's opaque id (tmux pane / tty / pid).
    pub id: String,
    /// Its most recent working directory, if it has told us.
    pub cwd: Option<String>,
    /// True between this shell's `start` and `end`.
    pub running: bool,
    /// The exit code of its last finished command.
    pub last_exit: Option<i32>,
    /// The last command it ran (already privacy-filtered by `emit`).
    pub last_cmd: Option<String>,
    /// Frame we last heard from it — used to prune shells that have gone away.
    last_seen: u64,
}

impl ShellState {
    fn new(id: String, frame: u64) -> ShellState {
        ShellState {
            id,
            cwd: None,
            running: false,
            last_exit: None,
            last_cmd: None,
            last_seen: frame,
        }
    }
}

pub struct App {
    /// Set true when the user asks to quit.
    pub should_quit: bool,
    /// Monotonic animation clock, bumped once per `step`.
    pub frame: u64,

    mode: Mode,
    game: Option<Box<dyn MiniGame>>,
    /// Highlighted entry in the game picker.
    menu_sel: usize,

    // --- look & feel ---
    theme: usize,

    // --- gamified shell ---
    xp: u32,
    best_streak: u32,

    // --- signals feeding the mood ---
    last_exit: Option<i32>,
    prev_exit: Option<i32>,
    commands_run: u32,
    success_streak: u32,
    /// Decaying "recent activity" counter; high when commands fire rapidly.
    burst: u32,
    idle_ticks: u32,
    /// Frame at which the *current* run began (any shell busy), or `None` when
    /// everything is idle. Drives how long a command has been running.
    run_started: Option<u64>,
    /// A small feed of the most recent commands (oldest first) with their exit
    /// codes, shown in the ambient pane so it visibly tracks your work.
    recent: Vec<(String, Option<i32>)>,

    // --- multi-shell awareness ---
    /// One entry per shell we've heard from in this scope.
    roster: Vec<ShellState>,
    /// The most recent working directory across all shells.
    cwd: Option<String>,
    /// When set, each new project dir picks a theme by hashing its path, so a
    /// repo keeps a consistent tint. Opt-in (`TERMLING_PROJECT_THEME=1`).
    project_theme: bool,

    // --- transient celebration ---
    /// An active flourish and how many frames it has left.
    flair: Option<(Flair, u32)>,

    // --- the buddy brain ---
    /// The local learner that watches your commands and talks back.
    buddy: Buddy,
    /// The current speech-bubble line and how many frames it has left.
    speech: Option<(Quip, u32)>,
}

/// How many recent commands the ambient feed remembers.
const RECENT_CAP: usize = 5;

impl App {
    pub fn new() -> App {
        let (xp, best_streak) = load_state();
        App {
            should_quit: false,
            frame: 0,
            mode: Mode::Ambient,
            game: None,
            menu_sel: 0,
            theme: 0,
            xp,
            best_streak,
            last_exit: None,
            prev_exit: None,
            commands_run: 0,
            success_streak: 0,
            burst: 0,
            idle_ticks: 0,
            run_started: None,
            recent: Vec::new(),
            roster: Vec::new(),
            cwd: None,
            project_theme: matches!(std::env::var("TERMLING_PROJECT_THEME").as_deref(), Ok("1")),
            flair: None,
            buddy: Buddy::load(),
            speech: None,
        }
    }

    // --- shell events (arriving over the socket) --------------------------

    /// Fold a real-shell event into the mood signals, XP, roster, and feed.
    pub fn apply_event(&mut self, ev: Event) {
        self.idle_ticks = 0;
        // Snapshot what the buddy brain needs before `ev` is consumed below.
        let (b_kind, b_cmd, b_code, b_dur, b_frame) = (
            ev.kind,
            ev.cmd.clone(),
            ev.code.unwrap_or(0),
            ev.dur_ms,
            self.frame,
        );
        let id = ev.shell.clone().unwrap_or_else(|| "shell".to_string());
        if let Some(pwd) = ev.pwd.clone() {
            self.set_cwd(pwd);
        }
        match ev.kind {
            Kind::Start => {
                let was_idle = !self.any_running();
                {
                    let frame = self.frame;
                    let s = self.shell_mut(&id);
                    s.running = true;
                    s.last_cmd = ev.cmd.clone();
                    if ev.pwd.is_some() {
                        s.cwd = ev.pwd.clone();
                    }
                    s.last_seen = frame;
                }
                if was_idle {
                    self.run_started = Some(self.frame);
                }
                // Remember the command line so we can label its result on `end`.
                if let Some(cmd) = ev.cmd {
                    self.push_recent(cmd, None);
                }
            }
            Kind::End => {
                let code = ev.code.unwrap_or(0);
                let was_long = self.run_ticks() > LONG_RUN_TICKS;
                {
                    let frame = self.frame;
                    let s = self.shell_mut(&id);
                    s.running = false;
                    s.last_exit = Some(code);
                    if ev.cmd.is_some() {
                        s.last_cmd = ev.cmd.clone();
                    }
                    if ev.pwd.is_some() {
                        s.cwd = ev.pwd.clone();
                    }
                    s.last_seen = frame;
                }
                if !self.any_running() {
                    self.run_started = None;
                }
                self.finish_recent(ev.cmd, code);

                let before_level = self.level();
                self.record_exit(code);
                // Pick the most notable thing to celebrate (level-up wins).
                if self.level() > before_level {
                    self.set_flair(Flair::LevelUp);
                } else if was_long {
                    self.set_flair(Flair::LongDone);
                } else if code == 0 && is_milestone(self.success_streak) {
                    self.set_flair(Flair::Milestone);
                }
                if was_long {
                    self.celebrate_long();
                }
            }
        }

        // Let the buddy brain learn from and react to this event. A `start` may
        // greet a new tool or predict; an `end` folds the outcome into the model
        // and reacts. Either can raise a fresh speech bubble.
        let quip = match b_kind {
            Kind::Start => self.buddy.on_start(b_cmd.as_deref(), b_frame),
            Kind::End => {
                // The shell hooks send the command text only on `start`; an `end`
                // carries just the exit code and duration. Fall back to the
                // command this shell started (the roster stashed it as `last_cmd`)
                // so the buddy can actually learn the tool and its outcome.
                let cmd = b_cmd.or_else(|| {
                    self.roster
                        .iter()
                        .find(|s| s.id == id)
                        .and_then(|s| s.last_cmd.clone())
                });
                self.buddy.on_end(cmd.as_deref(), b_code, b_dur, b_frame)
            }
        };
        if let Some(q) = quip {
            self.speech = Some((q, SPEECH_TICKS));
        }
    }

    /// Whether any shell in the roster is mid-command right now.
    pub fn any_running(&self) -> bool {
        self.roster.iter().any(|s| s.running)
    }

    /// How many frames the current run has been going (0 when idle).
    fn run_ticks(&self) -> u32 {
        self.run_started
            .map_or(0, |f| self.frame.saturating_sub(f) as u32)
    }

    /// The roster entry for `id`, creating it on first sight.
    fn shell_mut(&mut self, id: &str) -> &mut ShellState {
        if let Some(i) = self.roster.iter().position(|s| s.id == id) {
            return &mut self.roster[i];
        }
        self.roster
            .push(ShellState::new(id.to_string(), self.frame));
        self.roster.last_mut().unwrap()
    }

    /// Note the newest working directory; retint per-project when opted in.
    fn set_cwd(&mut self, pwd: String) {
        if self.cwd.as_deref() == Some(pwd.as_str()) {
            return;
        }
        if self.project_theme {
            self.theme = (hash_str(&pwd) as usize) % theme::THEMES.len();
        }
        self.cwd = Some(pwd);
    }

    /// Start a flourish (fixed lifetime); a fresh trigger replaces an older one.
    fn set_flair(&mut self, f: Flair) {
        self.flair = Some((f, FLAIR_TICKS));
    }

    /// Nudge the user back to a long job finishing: a terminal bell, plus an
    /// optional macOS notification when `TERMLING_NOTIFY=1`.
    fn celebrate_long(&self) {
        use std::io::Write;
        let _ = write!(std::io::stdout(), "\x07");
        let _ = std::io::stdout().flush();
        if matches!(std::env::var("TERMLING_NOTIFY").as_deref(), Ok("1")) {
            let script = "display notification \"a long command finished\" with title \"termling\"";
            // Fire and forget — never wait on it inside the render loop.
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .spawn();
        }
    }

    /// Fold a finished command's exit code into the mood signals and XP.
    fn record_exit(&mut self, code: i32) {
        self.commands_run += 1;
        // Bump the burst counter; it decays in `step`, so several quick commands
        // push it over the caffeinated threshold.
        self.burst = (self.burst + 3).min(BURST_CAP);
        if code == 0 {
            self.success_streak += 1;
            self.best_streak = self.best_streak.max(self.success_streak);
            // XP with a streak multiplier (capped) — the gamified shell.
            self.xp += 10 * self.success_streak.clamp(1, 5);
        } else {
            self.success_streak = 0;
        }
        self.prev_exit = self.last_exit;
        self.last_exit = Some(code);
    }

    /// Add a command to the recent feed (result unknown yet).
    fn push_recent(&mut self, cmd: String, code: Option<i32>) {
        self.recent.push((cmd, code));
        if self.recent.len() > RECENT_CAP {
            self.recent.remove(0);
        }
    }

    /// Stamp the outcome onto the feed. If the `start` gave us a command line we
    /// update that entry's code in place; otherwise we append a fresh one (some
    /// commands — e.g. the very first prompt — only send an `end`).
    fn finish_recent(&mut self, cmd: Option<String>, code: i32) {
        // The running command is the last entry still awaiting a result.
        if let Some(slot) = self.recent.last_mut() {
            if slot.1.is_none() {
                slot.1 = Some(code);
                if let Some(c) = cmd {
                    slot.0 = c;
                }
                return;
            }
        }
        self.push_recent(cmd.unwrap_or_default(), Some(code));
    }

    /// The recent-command feed (oldest first), for the ambient pane.
    pub fn recent(&self) -> &[(String, Option<i32>)] {
        &self.recent
    }

    /// The roster of shells we've heard from, for the ambient pane.
    pub fn shells(&self) -> &[ShellState] {
        &self.roster
    }

    /// The newest working directory across all shells, if known.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// The active celebratory flourish, if one is playing.
    pub fn flair(&self) -> Option<Flair> {
        self.flair.map(|(f, _)| f)
    }

    /// What the buddy is currently saying, if anything.
    pub fn speech(&self) -> Option<&Quip> {
        self.speech.as_ref().map(|(q, _)| q)
    }

    /// The buddy brain, for read-only introspection by the UI (learned tools, …).
    pub fn buddy(&self) -> &Buddy {
        &self.buddy
    }

    /// Persist XP + best streak. Called on quit.
    pub fn save(&self) {
        save_state(self.xp, self.best_streak);
    }

    // --- theming ----------------------------------------------------------

    pub fn theme(&self) -> Theme {
        theme::THEMES[self.theme]
    }

    /// Step to the next theme in the set.
    pub fn cycle_theme(&mut self) {
        self.theme = (self.theme + 1) % theme::THEMES.len();
    }

    // --- mode & game ------------------------------------------------------

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn game(&self) -> Option<&dyn MiniGame> {
        self.game.as_deref()
    }

    // -- the game picker --

    /// Open the arcade menu.
    pub fn open_menu(&mut self) {
        self.mode = Mode::Menu;
    }

    pub fn menu_sel(&self) -> usize {
        self.menu_sel
    }

    pub fn menu_prev(&mut self) {
        let n = GameKind::ALL.len();
        self.menu_sel = (self.menu_sel + n - 1) % n;
    }

    pub fn menu_next(&mut self) {
        self.menu_sel = (self.menu_sel + 1) % GameKind::ALL.len();
    }

    /// Start the highlighted game.
    pub fn menu_select(&mut self) {
        let kind = GameKind::ALL[self.menu_sel];
        self.game = Some(kind.make(self.current_character(), seed()));
        self.mode = Mode::Game;
    }

    /// Leave the picker, back to the ambient creature.
    pub fn exit_menu(&mut self) {
        self.mode = Mode::Ambient;
        self.idle_ticks = 0;
    }

    // -- an in-progress game --

    /// Leave the game, banking the score as XP; drops back to the picker.
    pub fn exit_game(&mut self) {
        if let Some(g) = self.game.take() {
            self.xp += g.score();
        }
        self.mode = Mode::Menu;
        self.idle_ticks = 0;
    }

    pub fn game_input(&mut self, i: Input) {
        if let Some(g) = &mut self.game {
            g.input(i);
        }
    }

    pub fn game_restart(&mut self) {
        if let Some(g) = &mut self.game {
            g.restart();
        }
    }

    // --- the tick / step loop ---------------------------------------------

    /// How long the main loop should wait before the next `step`. The game
    /// speeds up with level; the ambient view animates at a steady ~10fps.
    pub fn step_interval(&self) -> Duration {
        match (self.mode, &self.game) {
            (Mode::Game, Some(g)) if !g.is_over() => {
                let ms = 150u64.saturating_sub((g.level() as u64 - 1) * 12).max(55);
                Duration::from_millis(ms)
            }
            (Mode::Game, _) => Duration::from_millis(150),
            (Mode::Ambient | Mode::Menu, _) => Duration::from_millis(100),
        }
    }

    /// One cadence step: advance the animation clock, then either the game
    /// physics or the idle/mood bookkeeping.
    pub fn step(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        // A flourish fades regardless of mode so it can't get stuck on.
        if let Some((_, n)) = &mut self.flair {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.flair = None;
            }
        }
        // The speech bubble fades on the same clock, and regardless of mode.
        if let Some((_, n)) = &mut self.speech {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.speech = None;
            }
        }
        match self.mode {
            Mode::Game => {
                if let Some(g) = &mut self.game {
                    g.step();
                }
            }
            Mode::Ambient | Mode::Menu => {
                self.idle_ticks = self.idle_ticks.saturating_add(1);
                self.burst = self.burst.saturating_sub(1);
                // Forget shells we haven't heard from in a while (and aren't busy).
                let frame = self.frame;
                self.roster
                    .retain(|s| s.running || frame.saturating_sub(s.last_seen) < STALE_TICKS);
            }
        }
    }

    // --- derived views ----------------------------------------------------

    pub fn current_character(&self) -> Character {
        let cast = Character::CAST;
        cast[self.character_seed() % cast.len()]
    }

    /// A slowly-changing seed that selects which character is on stage: it
    /// advances every command and drifts over time, so the cast rotates as you
    /// work (and even while idle).
    pub fn character_seed(&self) -> usize {
        self.commands_run as usize + (self.frame as usize / 220)
    }

    pub fn level(&self) -> u32 {
        self.xp / 100 + 1
    }
    pub fn xp(&self) -> u32 {
        self.xp
    }
    /// XP progress within the current level, as (current, needed).
    pub fn level_progress(&self) -> (u32, u32) {
        (self.xp % 100, 100)
    }
    pub fn streak(&self) -> u32 {
        self.success_streak
    }
    pub fn best_streak(&self) -> u32 {
        self.best_streak
    }

    /// Snapshot for the mood engine. `running` is true while a command from your
    /// shell is in flight.
    pub fn mood(&self) -> Mood {
        Mood::from_signals(&Signals {
            running: self.any_running(),
            running_ticks: self.run_ticks(),
            last_exit: self.last_exit,
            prev_exit: self.prev_exit,
            commands_run: self.commands_run,
            success_streak: self.success_streak,
            burst: self.burst,
            idle_ticks: self.idle_ticks,
        })
    }
}

/// Ceiling for the burst counter so a long session can't peg it forever.
const BURST_CAP: u32 = 15;
/// A command running past this many ticks counts as "long" — a finish worth
/// celebrating (mirrors the mood engine's toil threshold).
const LONG_RUN_TICKS: u32 = 40;
/// How many frames a celebratory flourish stays on screen (~2s at ~10fps).
const FLAIR_TICKS: u32 = 20;
/// How many frames a buddy speech bubble lingers before fading (~4.5s at ~10fps),
/// long enough to read a short line without lagging behind rapid commands.
const SPEECH_TICKS: u32 = 45;
/// Frames without a peep before a shell is pruned from the roster (~60s).
const STALE_TICKS: u64 = 600;

/// Round success-streak lengths worth a little fanfare.
fn is_milestone(streak: u32) -> bool {
    matches!(streak, 3 | 5 | 10 | 20 | 50)
}

/// A tiny FNV-1a hash, enough to map a project path to a stable theme index.
fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// A varied-enough seed for the game PRNG.
fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

// --- persistence -----------------------------------------------------------
//
// Progress lives in one tiny file: "<xp> <best_streak>". No serde — same
// hand-rolled ethos as the rest of the crate.

/// `${XDG_STATE_HOME:-~/.local/state}/termling/state`.
fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("termling").join("state"))
}

/// Restore `(xp, best_streak)`, defaulting to zeros on any error.
fn load_state() -> (u32, u32) {
    let parse = || -> Option<(u32, u32)> {
        let text = std::fs::read_to_string(state_path()?).ok()?;
        let mut it = text.split_whitespace();
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    };
    parse().unwrap_or((0, 0))
}

fn save_state(xp: u32, best_streak: u32) {
    if let Some(path) = state_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, format!("{xp} {best_streak}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    #[test]
    fn a_start_then_end_updates_the_same_feed_entry() {
        let mut app = App::new();
        app.apply_event(Event::start(Some("cargo test".into())));
        assert_eq!(app.recent().len(), 1);
        assert_eq!(app.recent()[0], ("cargo test".into(), None));
        app.apply_event(Event::end(0));
        // Same entry, now stamped with its exit code — not a duplicate.
        assert_eq!(app.recent().len(), 1);
        assert_eq!(app.recent()[0].1, Some(0));
    }

    #[test]
    fn a_lone_end_still_records() {
        let mut app = App::new();
        app.apply_event(Event::end(1));
        assert_eq!(app.recent().len(), 1);
        assert_eq!(app.recent().last().unwrap().1, Some(1));
    }

    #[test]
    fn the_feed_is_capped() {
        let mut app = App::new();
        for i in 0..(RECENT_CAP + 3) {
            app.apply_event(Event::start(Some(format!("cmd{i}"))));
            app.apply_event(Event::end(0));
        }
        assert_eq!(app.recent().len(), RECENT_CAP);
        // Oldest entries dropped; the newest survives.
        assert_eq!(
            app.recent().last().unwrap().0,
            format!("cmd{}", RECENT_CAP + 2)
        );
    }

    #[test]
    fn two_shells_get_independent_roster_entries() {
        let mut app = App::new();
        let mut a = Event::start(Some("cargo build".into()));
        a.shell = Some("%1".into());
        a.pwd = Some("/proj/a".into());
        let mut b = Event::start(Some("npm test".into()));
        b.shell = Some("%2".into());
        b.pwd = Some("/proj/b".into());
        app.apply_event(a);
        app.apply_event(b);
        assert_eq!(app.shells().len(), 2);
        assert!(app.shells().iter().all(|s| s.running));
        assert!(app.any_running());
        // One finishes; the other stays busy.
        let mut done = Event::end(0);
        done.shell = Some("%1".into());
        app.apply_event(done);
        let a = app.shells().iter().find(|s| s.id == "%1").unwrap();
        assert!(!a.running);
        assert_eq!(a.last_exit, Some(0));
        assert!(app.any_running());
    }

    #[test]
    fn a_stale_shell_is_pruned_but_a_busy_one_survives() {
        let mut app = App::new();
        let mut idle = Event::end(0);
        idle.shell = Some("gone".into());
        app.apply_event(idle);
        let mut busy = Event::start(Some("sleep 999".into()));
        busy.shell = Some("here".into());
        app.apply_event(busy);
        for _ in 0..(STALE_TICKS + 5) {
            app.step();
        }
        let ids: Vec<&str> = app.shells().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["here"]);
    }

    #[test]
    fn a_long_command_finishing_raises_a_flourish() {
        let mut app = App::new();
        app.apply_event(Event::start(Some("cargo build".into())));
        // Let the clock run past the long-run threshold while it's busy.
        for _ in 0..(LONG_RUN_TICKS + 5) {
            app.step();
        }
        assert_eq!(app.mood(), Mood::Toiling);
        app.apply_event(Event::end(0));
        assert!(matches!(
            app.flair(),
            Some(Flair::LongDone | Flair::LevelUp)
        ));
        // And it fades on its own.
        for _ in 0..(FLAIR_TICKS + 1) {
            app.step();
        }
        assert!(app.flair().is_none());
    }

    #[test]
    fn a_command_makes_the_buddy_speak_then_the_bubble_fades() {
        let mut app = App::new();
        // A never-seen tool starting should raise a greeting bubble.
        app.apply_event(Event::start(Some("cargo build".into())));
        assert!(app.speech().is_some());
        // It fades on its own after its lifetime, like the flair.
        for _ in 0..(SPEECH_TICKS + 1) {
            app.step();
        }
        assert!(app.speech().is_none());
        // And the buddy learns the tool when the run ends (emit carries `cmd` on
        // `end`, which is where the outcome — and thus the learning — happens).
        let mut done = Event::end(0);
        done.cmd = Some("cargo build".into());
        app.apply_event(done);
        assert!(app.buddy().known_tools() >= 1);
    }

    #[test]
    fn the_buddy_learns_when_end_events_omit_the_command() {
        // The real shell hooks emit `--cmd` only on `start`; an `end` carries just
        // the exit code and duration. The buddy must still learn the tool by
        // pairing the end with the command this shell started.
        let mut app = App::new();
        app.apply_event(Event::start(Some("cargo build".into())));
        app.apply_event(Event::end(0)); // no cmd, exactly as the hooks emit
        assert_eq!(app.buddy().known_tools(), 1);
    }

    /// An end-to-end run of a plausible work session, fed exactly as the shell
    /// hooks emit it (command on `start`, a bare `end` carrying code + duration).
    /// Doubles as a readable demo: `cargo test -- --nocapture buddy_demo` prints
    /// the mood and the buddy's line after each command.
    #[test]
    fn buddy_demo_session_learns_and_reacts() {
        let mut app = App::new();
        // (command, exit code, duration ms)
        let session: &[(&str, i32, u64)] = &[
            ("cargo build", 0, 3_200),
            ("cargo test", 0, 8_100),
            ("git status", 0, 40),
            ("cargo build", 1, 1_500),
            ("cargo build", 0, 12_000), // slow one — twice the usual
            ("git commit", 0, 60),
            ("docker ps", 0, 300), // never seen before
        ];
        for (cmd, code, dur) in session {
            app.apply_event(Event::start(Some((*cmd).into())));
            let mut end = Event::end(*code);
            end.dur_ms = Some(*dur);
            app.apply_event(end);
            eprintln!(
                "{:>12}  exit {:<2}  mood {:<11}  buddy: {}",
                cmd,
                code,
                app.mood().label(),
                app.speech().map(|q| q.text.as_str()).unwrap_or("—"),
            );
            // A few ticks of "time passing" between commands, as in a real session.
            for _ in 0..3 {
                app.step();
            }
        }
        // It genuinely learned the tools it saw (cargo, git, docker).
        assert_eq!(app.buddy().known_tools(), 3);
    }

    #[test]
    fn a_failure_breaks_the_streak() {
        let mut app = App::new();
        for _ in 0..3 {
            app.apply_event(Event::end(0));
        }
        assert_eq!(app.streak(), 3);
        app.apply_event(Event::end(1));
        assert_eq!(app.streak(), 0);
        assert_eq!(app.best_streak(), 3);
    }
}
