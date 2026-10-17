//! The mini-game arcade: a small set of pluggable games that all reuse the
//! character cast and the active theme.
//!
//! Each game implements [`MiniGame`] — a tiny interface the app drives on a
//! timer (`step`) and feeds input to (`input`). Games render themselves to the
//! same colored [`Row`]s the doodle uses, tinted with the theme's [`ItemColors`].
//! Add a game: write a module implementing `MiniGame`, then add a [`GameKind`]
//! variant and wire it into `ALL` / `make`. The picker and HUD pick it up for
//! free.

use crate::doodle::{Character, Row, Seg};
use ratatui::style::Color;

mod blast;
mod bounce;
mod coil;
mod drop;
mod gapdive;
mod jump;
mod maze;
mod whack;

pub use blast::Blast;
pub use bounce::Bounce;
pub use coil::Coil;
pub use drop::DoodleDrop;
pub use gapdive::GapDive;
pub use jump::Jump;
pub use maze::Muncher;
pub use whack::LaneWhack;

/// A dim gray for static scenery (ground lines, empty holes) — theme-neutral.
pub const SCENERY: Color = Color::Rgb(95, 100, 110);

/// Merge a row of `(char, color)` cells into color-grouped segments — the atom
/// every game renders into. Keeps the span count small.
pub(crate) fn coalesce(cells: Vec<(char, Color)>) -> Row {
    let mut row: Row = Vec::new();
    let mut text = String::new();
    let mut color = Color::Reset;
    for (ch, col) in cells {
        if !text.is_empty() && col != color {
            row.push(Seg {
                text: std::mem::take(&mut text),
                color,
            });
        }
        color = col;
        text.push(ch);
    }
    if !text.is_empty() {
        row.push(Seg { text, color });
    }
    row
}

/// How hard a game plays. Bakes into each game's spawn rates / obstacle density
/// / starting lives — `Normal` reproduces the original, untuned behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    /// Lower-case label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Hard => "hard",
        }
    }

    /// Cycle to the next difficulty (wraps Hard → Easy).
    pub fn next(self) -> Difficulty {
        match self {
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Easy,
        }
    }

    /// A signed step for scaling a game's knobs: `-1` easier, `0` normal,
    /// `+1` harder. Games add this to spawn gaps, densities, and the like.
    pub fn notch(self) -> i32 {
        match self {
            Difficulty::Easy => -1,
            Difficulty::Normal => 0,
            Difficulty::Hard => 1,
        }
    }

    /// Starting-life adjustment vs. a game's base count (Easy +1, Hard −1).
    pub fn life_bonus(self) -> i32 {
        -self.notch()
    }
}

/// How fast the world ticks. Scales `App::step_interval` on top of each game's
/// own per-level speed curve — `Normal` leaves that curve untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    Calm,
    #[default]
    Normal,
    Fast,
}

impl Speed {
    /// Lower-case label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            Speed::Calm => "calm",
            Speed::Normal => "normal",
            Speed::Fast => "fast",
        }
    }

    /// Cycle to the next speed (wraps Fast → Calm).
    pub fn next(self) -> Speed {
        match self {
            Speed::Calm => Speed::Normal,
            Speed::Normal => Speed::Fast,
            Speed::Fast => Speed::Calm,
        }
    }

    /// Scale a tick interval in milliseconds: Calm stretches it (slower),
    /// Fast shortens it (quicker), Normal leaves it be.
    pub fn scale(self, ms: u64) -> u64 {
        match self {
            Speed::Calm => ms * 7 / 5,
            Speed::Normal => ms,
            Speed::Fast => ms * 7 / 10,
        }
    }
}

/// The player-chosen options a game is built with: how hard and how fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameOptions {
    pub difficulty: Difficulty,
    pub speed: Speed,
}

/// The three item colors a game paints with, supplied by the active theme.
#[derive(Clone, Copy)]
pub struct ItemColors {
    pub good: Color,
    pub gem: Color,
    pub bad: Color,
}

/// A single, genre-agnostic control signal. Games interpret what they need and
/// ignore the rest (a catcher only cares about Left/Right; a snake uses all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Left,
    Right,
    Up,
    Down,
    Action,
}

/// What every mini-game exposes to the app + UI.
pub trait MiniGame {
    /// Advance the world one tick.
    fn step(&mut self);
    /// Feed a control signal.
    fn input(&mut self, i: Input);
    /// Reset to a fresh round (same character).
    fn restart(&mut self);
    /// True once the round is lost.
    fn is_over(&self) -> bool;
    /// Current score (also banked as XP when leaving).
    fn score(&self) -> u32;
    /// Difficulty level — drives the tick speed in `App::step_interval`.
    fn level(&self) -> u32;
    /// Name of the character currently starring in the game (for the title).
    fn character_name(&self) -> &'static str;
    /// A short secondary HUD line (combo, lives already shown separately, etc.).
    fn hud(&self) -> String;
    /// Lives remaining, if the game has them (drives the ♥ bar). `None` = hide.
    fn lives(&self) -> Option<u32>;
    /// One-line controls hint for the footer.
    fn controls(&self) -> &'static str;
    /// Render the play-field to colored rows.
    fn render(&self, colors: ItemColors) -> Vec<Row>;
}

/// The catalogue of playable games, shown in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameKind {
    Drop,
    GapDive,
    Coil,
    Whack,
    Jump,
    Blast,
    Muncher,
    Bounce,
}

impl GameKind {
    /// Every game, in picker order. Add a variant here to list it.
    pub const ALL: &'static [GameKind] = &[
        GameKind::Drop,
        GameKind::GapDive,
        GameKind::Coil,
        GameKind::Whack,
        GameKind::Jump,
        GameKind::Blast,
        GameKind::Muncher,
        GameKind::Bounce,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GameKind::Drop => "doodle drop",
            GameKind::GapDive => "gap dive",
            GameKind::Coil => "coil",
            GameKind::Whack => "lane whack",
            GameKind::Jump => "doodle dash",
            GameKind::Blast => "star blast",
            GameKind::Muncher => "muncher",
            GameKind::Bounce => "brick bounce",
        }
    }

    /// One-line pitch for the picker.
    pub fn tagline(self) -> &'static str {
        match self {
            GameKind::Drop => "catch stars & gems, dodge bombs",
            GameKind::GapDive => "thread the gap in falling walls",
            GameKind::Coil => "grow the trail, don't bite yourself",
            GameKind::Whack => "smack the moles, dodge the bombs",
            GameKind::Jump => "hop the cacti, run forever",
            GameKind::Blast => "shoot the invaders before they land",
            GameKind::Muncher => "gobble pellets, dodge the ghost",
            GameKind::Bounce => "clear the bricks, don't drop the ball",
        }
    }

    /// Build a fresh instance for the given character, tuned by `opts`.
    pub fn make(self, character: Character, seed: u64, opts: GameOptions) -> Box<dyn MiniGame> {
        match self {
            GameKind::Drop => Box::new(DoodleDrop::new(character, seed, opts)),
            GameKind::GapDive => Box::new(GapDive::new(character, seed, opts)),
            GameKind::Coil => Box::new(Coil::new(character, seed, opts)),
            GameKind::Whack => Box::new(LaneWhack::new(character, seed, opts)),
            GameKind::Jump => Box::new(Jump::new(character, seed, opts)),
            GameKind::Blast => Box::new(Blast::new(character, seed, opts)),
            GameKind::Muncher => Box::new(Muncher::new(character, seed, opts)),
            GameKind::Bounce => Box::new(Bounce::new(character, seed, opts)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_cycles_and_defaults_to_normal() {
        assert_eq!(Difficulty::default(), Difficulty::Normal);
        assert_eq!(Difficulty::Easy.next(), Difficulty::Normal);
        assert_eq!(Difficulty::Normal.next(), Difficulty::Hard);
        assert_eq!(Difficulty::Hard.next(), Difficulty::Easy);
        // The notch and life bonus move in opposite directions.
        assert_eq!(Difficulty::Easy.notch(), -1);
        assert_eq!(Difficulty::Hard.life_bonus(), -1);
    }

    #[test]
    fn speed_cycles_and_scales_the_tick() {
        assert_eq!(Speed::default(), Speed::Normal);
        assert_eq!(Speed::Calm.next(), Speed::Normal);
        assert_eq!(Speed::Fast.next(), Speed::Calm);
        // Normal is a no-op; Calm slows the tick, Fast quickens it.
        assert_eq!(Speed::Normal.scale(100), 100);
        assert!(Speed::Calm.scale(100) > 100);
        assert!(Speed::Fast.scale(100) < 100);
    }

    #[test]
    fn game_options_defaults_to_normal_normal() {
        let o = GameOptions::default();
        assert_eq!(o.difficulty, Difficulty::Normal);
        assert_eq!(o.speed, Speed::Normal);
    }
}
