//! "Lane Whack" — a five-lane reflex game. Moles pop out of holes: good ones
//! (`O`) and rare gems (`*`) want whacking, bombs (`X`) do not. Slide your mallet
//! (`^`) to a lane and swing (space) before the target expires. Whack fast and
//! chain hits for a combo; let a good mole time out — or clobber a bomb — and you
//! lose a life. It speeds up and the moles get twitchier as you climb.
//!
//! One `step()` = one tick: moles age, and now and then a new one pops.

use super::{coalesce, GameOptions, Input, ItemColors, MiniGame, SCENERY};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;

/// Five lane centers across the field.
const LANES: [usize; 5] = [2, 6, 10, 14, 18];
const MOLE_ROW: usize = 4;
const HOLE_ROW: usize = 5;
const MALLET_ROW: usize = 8;
const START_LIVES: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Good,
    Gem,
    Bomb,
}

struct Mole {
    kind: Kind,
    ttl: u32,
}

pub struct LaneWhack {
    character: Character,
    lanes: [Option<Mole>; 5],
    cursor: usize,
    score: u32,
    lives: u32,
    combo: u32,
    best_combo: u32,
    hits: u32,
    spawn_timer: u32,
    over: bool,
    opts: GameOptions,
    rng: Rng,
}

impl LaneWhack {
    pub fn new(character: Character, seed: u64, opts: GameOptions) -> LaneWhack {
        let lives = (START_LIVES as i32 + opts.difficulty.life_bonus()).max(1) as u32;
        LaneWhack {
            character,
            lanes: std::array::from_fn(|_| None),
            cursor: 2,
            score: 0,
            lives,
            combo: 0,
            best_combo: 0,
            hits: 0,
            spawn_timer: 2,
            over: false,
            opts,
            rng: Rng::new(seed),
        }
    }

    /// How long a fresh mole lives, in ticks — shorter as the level climbs.
    fn lifetime(level: u32) -> u32 {
        (30i32 - 3 * level as i32).clamp(8, 30) as u32
    }

    /// Ticks between pops — tighter as the level climbs.
    fn spawn_gap(level: u32) -> u32 {
        (18i32 - 2 * level as i32).clamp(5, 18) as u32
    }

    fn lose_life(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        self.combo = 0;
        if self.lives == 0 {
            self.over = true;
        }
    }

    /// Swing the mallet at the current lane.
    fn whack(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();
        let Some(m) = self.lanes[self.cursor].take() else {
            return; // whiffed at an empty hole — no penalty, just no reward
        };
        match m.kind {
            Kind::Bomb => self.lose_life(),
            Kind::Good | Kind::Gem => {
                let base = if m.kind == Kind::Gem { 30 } else { 10 };
                let combo_mul = if m.kind == Kind::Gem { 3 } else { 2 };
                // Reward hitting while plenty of life remains on the mole.
                self.score += base + m.ttl + self.combo * combo_mul + level;
                self.combo += 1;
                self.best_combo = self.best_combo.max(self.combo);
                self.hits += 1;
            }
        }
    }

    fn spawn(&mut self, level: u32) {
        // Pick a currently-empty lane.
        let empties: Vec<usize> = (0..5).filter(|&i| self.lanes[i].is_none()).collect();
        if empties.is_empty() {
            return;
        }
        let lane = empties[self.rng.below(empties.len())];
        // Difficulty shifts how often a bomb pops instead of a scorable mole.
        let bomb_pct = ((20 + level * 4).min(50) as i32 + self.opts.difficulty.notch() * 8)
            .clamp(5, 70) as u32;
        let kind = if self.rng.chance(bomb_pct, 100) {
            Kind::Bomb
        } else if self.rng.chance(1, 7) {
            Kind::Gem
        } else {
            Kind::Good
        };
        self.lanes[lane] = Some(Mole {
            kind,
            ttl: Self::lifetime(level),
        });
    }
}

impl MiniGame for LaneWhack {
    fn input(&mut self, i: Input) {
        match i {
            Input::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            Input::Right => {
                if self.cursor < LANES.len() - 1 {
                    self.cursor += 1;
                }
            }
            Input::Action => self.whack(),
            _ => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = LaneWhack::new(self.character, seed, self.opts);
    }

    fn score(&self) -> u32 {
        self.score
    }
    fn lives(&self) -> Option<u32> {
        Some(self.lives)
    }
    fn is_over(&self) -> bool {
        self.over
    }
    fn controls(&self) -> &'static str {
        "← → aim  ·  space whack  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("combo x{}   best x{}", self.combo, self.best_combo)
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.hits / 12 + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();

        // Age the moles; a good one that times out is a missed hit.
        let mut missed = 0u32;
        for slot in self.lanes.iter_mut() {
            if let Some(m) = slot {
                m.ttl = m.ttl.saturating_sub(1);
                if m.ttl == 0 {
                    if matches!(m.kind, Kind::Good | Kind::Gem) {
                        missed += 1;
                    }
                    *slot = None;
                }
            }
        }
        for _ in 0..missed {
            self.lose_life();
        }
        if self.over {
            return;
        }

        self.spawn_timer = self.spawn_timer.saturating_sub(1);
        if self.spawn_timer == 0 {
            self.spawn(level);
            self.spawn_timer = Self::spawn_gap(level);
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        for &c in &LANES {
            // A little cup for each hole: \_/
            cells[HOLE_ROW][c - 1] = ('\\', SCENERY);
            cells[HOLE_ROW][c] = ('_', SCENERY);
            cells[HOLE_ROW][c + 1] = ('/', SCENERY);
            // Faint lane guides down to the mallet row.
            for row in cells.iter_mut().take(MALLET_ROW).skip(HOLE_ROW + 1) {
                row[c] = ('.', SCENERY);
            }
        }

        // Moles sitting in their holes.
        for (i, slot) in self.lanes.iter().enumerate() {
            if let Some(m) = slot {
                let (ch, col) = match m.kind {
                    Kind::Good => ('O', colors.good),
                    Kind::Gem => ('*', colors.gem),
                    Kind::Bomb => ('X', colors.bad),
                };
                cells[MOLE_ROW][LANES[i]] = (ch, col);
            }
        }

        // The mallet under the selected lane.
        cells[MALLET_ROW][LANES[self.cursor]] = ('^', self.character.body_color());

        // Ground line.
        for c in cells[GH - 1].iter_mut() {
            *c = ('═', SCENERY);
        }

        cells.into_iter().map(coalesce).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> GameOptions {
        GameOptions::default()
    }

    fn colors() -> ItemColors {
        ItemColors {
            good: Color::Reset,
            gem: Color::Reset,
            bad: Color::Reset,
        }
    }

    fn mole(kind: Kind, ttl: u32) -> Mole {
        Mole { kind, ttl }
    }

    #[test]
    fn whacking_a_good_mole_scores_and_builds_combo() {
        let mut g = LaneWhack::new(Character::Blob, 1, opts());
        g.cursor = 2;
        g.lanes[2] = Some(mole(Kind::Good, 20));
        g.input(Input::Action);
        assert!(g.score >= 30); // 10 base + 20 ttl
        assert_eq!(g.combo, 1);
        assert_eq!(g.hits, 1);
        assert!(g.lanes[2].is_none());
    }

    #[test]
    fn whacking_a_bomb_costs_a_life() {
        let mut g = LaneWhack::new(Character::Robot, 2, opts());
        g.cursor = 0;
        g.lanes[0] = Some(mole(Kind::Bomb, 20));
        g.combo = 5;
        g.input(Input::Action);
        assert_eq!(g.lives, START_LIVES - 1);
        assert_eq!(g.combo, 0);
    }

    #[test]
    fn a_good_mole_timing_out_costs_a_life() {
        let mut g = LaneWhack::new(Character::Cat, 3, opts());
        g.lanes[1] = Some(mole(Kind::Good, 1));
        g.spawn_timer = 999;
        g.step();
        assert_eq!(g.lives, START_LIVES - 1);
        assert!(g.lanes[1].is_none());
    }

    #[test]
    fn a_bomb_timing_out_is_safe() {
        let mut g = LaneWhack::new(Character::Fox, 4, opts());
        g.lanes[3] = Some(mole(Kind::Bomb, 1));
        g.spawn_timer = 999;
        g.step();
        assert_eq!(g.lives, START_LIVES);
        assert!(g.lanes[3].is_none());
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut g = LaneWhack::new(Character::Ghost, 5, opts());
        for _ in 0..10 {
            g.input(Input::Left);
        }
        assert_eq!(g.cursor, 0);
        for _ in 0..10 {
            g.input(Input::Right);
        }
        assert_eq!(g.cursor, LANES.len() - 1);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let mut g = LaneWhack::new(Character::TubeBoy, 6, opts());
        g.lanes[0] = Some(mole(Kind::Good, 10));
        g.lanes[4] = Some(mole(Kind::Bomb, 10));
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = LaneWhack::new(Character::Jellyfish, 0xBEEF, opts());
        for i in 0..5000 {
            match i % 5 {
                0 => g.input(Input::Left),
                1 => g.input(Input::Right),
                2 => g.input(Input::Action),
                _ => {}
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
        }
    }
}
