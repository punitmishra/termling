//! The creature's mood — the little brain behind the doodle.
//!
//! A `Mood` is derived from what just happened in the shell. Keeping the
//! derivation in one place (`from_signals`) means the doodle layer never has to
//! reason about shell state — it just asks "which mood?" and draws it. Moods are
//! deliberately orthogonal to *characters* (see `doodle.rs`): any character can
//! wear any mood, so adding one here instantly works across the whole cast.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// Idle for a long time — asleep.
    Sleepy,
    /// Idle for a little while — twiddling thumbs.
    Bored,
    /// Fresh start / waiting for the first command.
    Curious,
    /// A command is running right now.
    Focused,
    /// A command has been running a *long* time — a big build or test run.
    Toiling,
    /// Commands firing in rapid succession — wired.
    Caffeinated,
    /// Last command exited 0.
    Happy,
    /// Recovered: a success right after a failure.
    Proud,
    /// Several successes in a row.
    Excited,
    /// Last command exited non-zero.
    Confused,
}

/// Everything the mood cares about, gathered from the app each frame.
pub struct Signals {
    pub running: bool,
    /// Ticks the current command has been running (0 when nothing runs).
    pub running_ticks: u32,
    pub last_exit: Option<i32>,
    pub prev_exit: Option<i32>,
    pub commands_run: u32,
    pub success_streak: u32,
    /// Decaying counter of recent command activity — high = rapid-fire.
    pub burst: u32,
    /// Ticks since the last keypress or command finished.
    pub idle_ticks: u32,
}

impl Mood {
    /// The single source of truth mapping signals → mood. Order is priority:
    /// the first matching rule wins.
    pub fn from_signals(s: &Signals) -> Mood {
        if s.running {
            return if s.running_ticks > TOIL_TICKS {
                Mood::Toiling
            } else {
                Mood::Focused
            };
        }
        if s.commands_run == 0 {
            return if s.idle_ticks > SLEEPY_TICKS {
                Mood::Sleepy
            } else {
                Mood::Curious
            };
        }
        if s.idle_ticks > SLEEPY_TICKS {
            return Mood::Sleepy;
        }
        if s.idle_ticks > BORED_TICKS {
            return Mood::Bored;
        }
        if s.burst >= CAFFEINE_BURST {
            return Mood::Caffeinated;
        }
        if matches!(s.last_exit, Some(code) if code != 0) {
            return Mood::Confused;
        }
        // From here on the last command succeeded (exit 0).
        if matches!(s.prev_exit, Some(code) if code != 0) {
            return Mood::Proud; // just recovered from an error
        }
        if s.success_streak >= EXCITED_STREAK {
            return Mood::Excited;
        }
        Mood::Happy
    }

    pub fn label(self) -> &'static str {
        match self {
            Mood::Sleepy => "sleepy",
            Mood::Bored => "bored",
            Mood::Curious => "curious",
            Mood::Focused => "focused",
            Mood::Toiling => "toiling",
            Mood::Caffeinated => "caffeinated",
            Mood::Happy => "happy",
            Mood::Proud => "proud",
            Mood::Excited => "excited",
            Mood::Confused => "confused",
        }
    }
}

/// Idle ticks before the creature gets bored, then falls asleep. ~1 tick = one poll.
const BORED_TICKS: u32 = 25;
const SLEEPY_TICKS: u32 = 70;
/// Ticks a single command must run before the creature starts *toiling* (~4s at the
/// ambient ~10fps cadence) — long enough that a quick command stays `focused`.
const TOIL_TICKS: u32 = 40;
/// Burst level that counts as rapid-fire (see `App` for how it accrues/decays).
const CAFFEINE_BURST: u32 = 8;
/// Consecutive successes that tip into `Excited`.
const EXCITED_STREAK: u32 = 3;
