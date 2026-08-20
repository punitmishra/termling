//! "Doodle Drop" — a tiny catch game that reuses the character cast.
//!
//! Your current character becomes a catcher at the bottom of the field. Stars
//! (★, +10) and gems (◆, +30) fall from the top; catch them to score and build
//! a combo. Bombs (●) cost a life. Miss a good drop and the combo resets. It
//! gets faster as your score climbs (the main loop shortens the tick interval
//! with `level`).
//!
//! One `step()` = one row of falling. The main loop calls it on a timer, so the
//! game's speed is just how often `step` runs — see `App::step_interval`.

use super::{coalesce, Input, ItemColors, MiniGame};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

/// Play-field size (columns × rows), centered inside the doodle pane.
pub const GW: usize = 22;
pub const GH: usize = 12;
/// Width of the catcher sprite.
const PLAYER_W: usize = 3;
const START_LIVES: u32 = 3;

/// Item glyphs — filled shapes read as one system. (Rendered width-1 in typical
/// terminals; the play-field assumes that.)
const STAR: char = '★';
const GEM: char = '◆';
const BOMB: char = '●';

struct Item {
    x: usize,
    y: usize,
    bad: bool,
    gem: bool,
}

pub struct DoodleDrop {
    character: Character,
    px: usize,
    items: Vec<Item>,
    score: u32,
    lives: u32,
    combo: u32,
    best_combo: u32,
    spawn_timer: u32,
    over: bool,
    rng: Rng,
}

impl DoodleDrop {
    pub fn new(character: Character, seed: u64) -> DoodleDrop {
        DoodleDrop {
            character,
            px: GW / 2 - 1,
            items: Vec::new(),
            score: 0,
            lives: START_LIVES,
            combo: 0,
            best_combo: 0,
            spawn_timer: 2,
            over: false,
            rng: Rng::new(seed),
        }
    }

    fn move_left(&mut self) {
        if !self.over && self.px > 0 {
            self.px -= 1;
        }
    }

    fn move_right(&mut self) {
        if !self.over && self.px + PLAYER_W < GW {
            self.px += 1;
        }
    }

    fn spawn(&mut self, level: u32) {
        // Bombs get more common with level, capped so it stays fair.
        let bad = self.rng.chance((25 + level * 4).min(55), 100);
        let gem = !bad && self.rng.chance(1, 6);
        let x = self.rng.below(GW);
        self.items.push(Item { x, y: 0, bad, gem });
    }
}

impl MiniGame for DoodleDrop {
    fn input(&mut self, i: Input) {
        match i {
            Input::Left => self.move_left(),
            Input::Right => self.move_right(),
            _ => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = DoodleDrop::new(self.character, seed);
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
        "← → move  ·  esc back"
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }
    fn hud(&self) -> String {
        format!("combo x{}   best x{}", self.combo, self.best_combo)
    }

    /// Difficulty level, derived from score. Drives fall speed + spawn rate.
    fn level(&self) -> u32 {
        self.score / 120 + 1
    }

    /// Advance the world one row.
    fn step(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();

        // Fall, then resolve anything that reached the catcher's row.
        let mut kept = Vec::with_capacity(self.items.len());
        for mut it in std::mem::take(&mut self.items) {
            it.y += 1;
            if it.y < GH - 1 {
                kept.push(it);
                continue;
            }
            let caught = it.x >= self.px && it.x < self.px + PLAYER_W;
            if caught {
                if it.bad {
                    self.lives = self.lives.saturating_sub(1);
                    self.combo = 0;
                } else {
                    let base = if it.gem { 30 } else { 10 };
                    self.score += base + self.combo * 2;
                    self.combo += 1;
                    self.best_combo = self.best_combo.max(self.combo);
                }
            } else if !it.bad {
                self.combo = 0; // fumbled a good drop
            }
        }
        self.items = kept;

        if self.lives == 0 {
            self.over = true;
            return;
        }

        // Spawn on a countdown that tightens with level.
        self.spawn_timer = self.spawn_timer.saturating_sub(1);
        if self.spawn_timer == 0 {
            self.spawn(level);
            self.spawn_timer = (6u32.saturating_sub(level)).max(2);
        }
    }

    /// Render the field to colored rows, tinting items with the theme's colors.
    /// The catcher sits on the bottom row.
    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let space = Color::Reset;
        let mut cells = vec![vec![(' ', space); GW]; GH];

        for it in &self.items {
            if it.y < GH - 1 {
                let (ch, col) = if it.bad {
                    (BOMB, colors.bad)
                } else if it.gem {
                    (GEM, colors.gem)
                } else {
                    (STAR, colors.good)
                };
                cells[it.y][it.x] = (ch, col);
            }
        }

        let sprite = player_sprite(self.character);
        let pc = self.character.body_color();
        for (i, ch) in sprite.iter().enumerate() {
            if self.px + i < GW {
                cells[GH - 1][self.px + i] = (*ch, pc);
            }
        }

        cells.into_iter().map(coalesce).collect()
    }
}

/// A 3-char catcher sprite per character — a nod to each creature.
fn player_sprite(c: Character) -> [char; PLAYER_W] {
    match c {
        Character::Cat => ['=', '^', '='],
        Character::Robot => ['[', 'o', ']'],
        Character::Ghost => ['~', 'o', '~'],
        Character::Fox => ['/', 'v', '\\'],
        _ => ['\\', 'o', '/'],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An item one row above the catcher, aligned with the player.
    fn drop_on_player(g: &DoodleDrop, bad: bool, gem: bool) -> Item {
        Item {
            x: g.px + 1,
            y: GH - 2,
            bad,
            gem,
        }
    }

    #[test]
    fn catching_a_star_scores_and_builds_combo() {
        let mut g = DoodleDrop::new(Character::Blob, 42);
        g.items = vec![drop_on_player(&g, false, false)];
        g.step();
        assert_eq!(g.score, 10);
        assert_eq!(g.combo, 1);
        assert!(g.items.is_empty(), "caught item should be consumed");
    }

    #[test]
    fn catching_a_bomb_costs_a_life_and_breaks_combo() {
        let mut g = DoodleDrop::new(Character::Robot, 7);
        g.combo = 3;
        g.items = vec![drop_on_player(&g, true, false)];
        g.step();
        assert_eq!(g.lives, START_LIVES - 1);
        assert_eq!(g.combo, 0);
    }

    #[test]
    fn missing_a_good_drop_resets_combo() {
        let mut g = DoodleDrop::new(Character::Cat, 99);
        g.combo = 4;
        // A star far from the player (column 0) — the catcher starts centered.
        g.items = vec![Item {
            x: 0,
            y: GH - 2,
            bad: false,
            gem: false,
        }];
        g.step();
        assert_eq!(g.combo, 0);
        assert_eq!(g.score, 0);
    }

    #[test]
    fn losing_all_lives_ends_the_game() {
        let mut g = DoodleDrop::new(Character::Fox, 5);
        g.lives = 1;
        g.items = vec![drop_on_player(&g, true, false)];
        g.step();
        assert!(g.is_over());
    }

    #[test]
    fn movement_stays_in_bounds() {
        let mut g = DoodleDrop::new(Character::Ghost, 1);
        for _ in 0..GW * 2 {
            g.move_left();
        }
        assert_eq!(g.px, 0);
        for _ in 0..GW * 2 {
            g.move_right();
        }
        assert_eq!(g.px, GW - PLAYER_W);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = DoodleDrop::new(Character::Blob, 3);
        let colors = ItemColors {
            good: Color::Reset,
            gem: Color::Reset,
            bad: Color::Reset,
        };
        let rows = g.render(colors);
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic_and_keep_invariants() {
        let mut g = DoodleDrop::new(Character::Blob, 0xDEAD_BEEF);
        for i in 0..3000 {
            if i % 3 == 0 {
                g.move_right();
            }
            if i % 7 == 0 {
                g.move_left();
            }
            g.step();
            assert!(g.px + PLAYER_W <= GW);
            assert!(g.items.iter().all(|it| it.y < GH));
        }
    }
}
