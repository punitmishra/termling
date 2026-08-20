//! "Star Blast" — a tiny fixed-shooter. Your ship (`A`) holds the bottom row and
//! slides left/right; `Space` fires a shot (`|`) straight up. Invaders (`W`, and
//! the rarer golden `@`) drift down from the top — pop them for points, but let
//! one reach your row and you lose a life. It quickens with every wave.
//!
//! One `step()` = one tick: shots rise, and every so often the invaders drop.

use super::{coalesce, Input, ItemColors, MiniGame};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;
const PLAYER_ROW: usize = 11;
const START_LIVES: u32 = 3;
/// Ticks between shots.
const FIRE_CD: u32 = 2;

struct Enemy {
    x: usize,
    y: usize,
    gem: bool,
}

pub struct Blast {
    character: Character,
    px: usize,
    bullets: Vec<(usize, usize)>,
    enemies: Vec<Enemy>,
    fire_cd: u32,
    tick: u32,
    spawn_timer: u32,
    score: u32,
    lives: u32,
    over: bool,
    rng: Rng,
}

impl Blast {
    pub fn new(character: Character, seed: u64) -> Blast {
        Blast {
            character,
            px: GW / 2,
            bullets: Vec::new(),
            enemies: Vec::new(),
            fire_cd: 0,
            tick: 0,
            spawn_timer: 2,
            score: 0,
            lives: START_LIVES,
            over: false,
            rng: Rng::new(seed),
        }
    }

    /// Invaders step down one row every this-many ticks — tighter with level.
    fn drop_period(level: u32) -> u32 {
        (7u32.saturating_sub(level)).max(2)
    }

    fn fire(&mut self) {
        if !self.over && self.fire_cd == 0 {
            self.bullets.push((self.px, PLAYER_ROW - 1));
            self.fire_cd = FIRE_CD;
        }
    }

    fn spawn(&mut self, level: u32) {
        let x = self.rng.below(GW);
        let gem = self.rng.chance(1, 8);
        self.enemies.push(Enemy { x, y: 0, gem });
        self.spawn_timer = (9u32.saturating_sub(level)).max(3);
    }
}

impl MiniGame for Blast {
    fn input(&mut self, i: Input) {
        match i {
            Input::Left if self.px > 0 => self.px -= 1,
            Input::Right if self.px + 1 < GW => self.px += 1,
            Input::Action => self.fire(),
            _ => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = Blast::new(self.character, seed);
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
        "← → move  ·  space fire  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("invaders {}", self.enemies.len())
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.score / 100 + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();
        self.tick = self.tick.wrapping_add(1);
        self.fire_cd = self.fire_cd.saturating_sub(1);

        // Shots rise.
        for b in &mut self.bullets {
            b.1 = b.1.saturating_sub(1);
        }

        // Invaders descend on their cadence.
        if self.tick.is_multiple_of(Self::drop_period(level)) {
            for e in &mut self.enemies {
                e.y += 1;
            }
        }

        // Resolve shots vs invaders: each shot hits at most one invader; a shot
        // that misses survives only while still on-screen (row > 0).
        let mut dead = vec![false; self.enemies.len()];
        let mut surviving = Vec::with_capacity(self.bullets.len());
        for &(bx, by) in &self.bullets {
            let mut consumed = false;
            for (ei, e) in self.enemies.iter().enumerate() {
                if !dead[ei] && e.x == bx && e.y == by {
                    dead[ei] = true;
                    self.score += if e.gem { 50 } else { 10 };
                    consumed = true;
                    break;
                }
            }
            if !consumed && by > 0 {
                surviving.push((bx, by));
            }
        }
        self.bullets = surviving;
        let mut ei = 0;
        self.enemies.retain(|_| {
            let d = dead[ei];
            ei += 1;
            !d
        });

        // Breaches: an invader reaching the ship's row costs a life.
        let mut breaches = 0u32;
        self.enemies.retain(|e| {
            if e.y >= PLAYER_ROW {
                breaches += 1;
                false
            } else {
                true
            }
        });
        for _ in 0..breaches {
            self.lives = self.lives.saturating_sub(1);
        }
        if self.lives == 0 {
            self.over = true;
            return;
        }

        self.spawn_timer = self.spawn_timer.saturating_sub(1);
        if self.spawn_timer == 0 {
            self.spawn(level);
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        for e in &self.enemies {
            if e.y < GH {
                let (ch, col) = if e.gem {
                    ('@', colors.gem)
                } else {
                    ('W', colors.bad)
                };
                cells[e.y][e.x] = (ch, col);
            }
        }
        for &(x, y) in &self.bullets {
            if y < GH {
                cells[y][x] = ('|', colors.good);
            }
        }
        cells[PLAYER_ROW][self.px] = ('A', self.character.body_color());

        cells.into_iter().map(coalesce).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> ItemColors {
        ItemColors {
            good: Color::Reset,
            gem: Color::Reset,
            bad: Color::Reset,
        }
    }

    #[test]
    fn a_shot_destroys_an_invader() {
        let mut g = Blast::new(Character::Blob, 1);
        g.enemies = vec![Enemy {
            x: 5,
            y: 4,
            gem: false,
        }];
        g.bullets = vec![(5, 5)]; // one row below the invader, rising into it
        g.spawn_timer = 999;
        g.step();
        assert!(g.enemies.is_empty());
        assert_eq!(g.score, 10);
    }

    #[test]
    fn an_invader_reaching_the_ship_costs_a_life() {
        let mut g = Blast::new(Character::Fox, 2);
        g.enemies = vec![Enemy {
            x: 5,
            y: PLAYER_ROW,
            gem: false,
        }];
        g.spawn_timer = 999;
        g.step();
        assert_eq!(g.lives, START_LIVES - 1);
        assert!(g.enemies.is_empty());
    }

    #[test]
    fn fire_respects_cooldown() {
        let mut g = Blast::new(Character::Cat, 3);
        g.fire();
        g.fire(); // blocked by cooldown
        assert_eq!(g.bullets.len(), 1);
    }

    #[test]
    fn movement_stays_in_bounds() {
        let mut g = Blast::new(Character::Ghost, 4);
        for _ in 0..GW * 2 {
            g.input(Input::Left);
        }
        assert_eq!(g.px, 0);
        for _ in 0..GW * 2 {
            g.input(Input::Right);
        }
        assert_eq!(g.px, GW - 1);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let mut g = Blast::new(Character::Robot, 5);
        g.enemies = vec![Enemy {
            x: 2,
            y: 3,
            gem: true,
        }];
        g.bullets = vec![(4, 6)];
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = Blast::new(Character::Jellyfish, 0xB1A5);
        for i in 0..5000 {
            match i % 4 {
                0 => g.input(Input::Left),
                1 => g.input(Input::Right),
                _ => g.input(Input::Action),
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
        }
    }
}
