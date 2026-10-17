//! "Gap Dive" — a canyon runner. Full-width walls scroll down; slide your
//! bottom-row glider left/right to thread the single gap in each wall (and greedily
//! scoop the gem sitting inside it) before the wall reaches you. One mistake ends
//! the run. It gets faster, the gap narrows, and walls arrive sooner as you climb.
//!
//! One `step()` = one row of world scroll.

use super::{coalesce, GameOptions, Input, ItemColors, MiniGame, SCENERY};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;
const PLAYER_W: usize = 3;
/// The glider's fixed row (one above the decorative ground).
const PLAYER_ROW: usize = 10;

const WALL: char = '█';
const GEM: char = '◆';
const GROUND: char = '═';

struct Wall {
    y: usize,
    gap_start: usize,
    gap_w: usize,
    gem_col: Option<usize>,
    gem_taken: bool,
    scored: bool,
}

pub struct GapDive {
    character: Character,
    px: usize,
    walls: Vec<Wall>,
    prev_gap: usize,
    score: u32,
    spawn_timer: usize,
    over: bool,
    opts: GameOptions,
    rng: Rng,
}

impl GapDive {
    pub fn new(character: Character, seed: u64, opts: GameOptions) -> GapDive {
        GapDive {
            character,
            px: GW / 2 - 1,
            walls: Vec::new(),
            prev_gap: (GW - 6) / 2,
            score: 0,
            spawn_timer: 1,
            over: false,
            opts,
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
        let period = (6usize.saturating_sub(level as usize)).max(3);
        // Difficulty widens or narrows the gap you must thread.
        let notch = self.opts.difficulty.notch();
        let floor = (4 - notch).max(3);
        let gw = (7i32 - level as i32 - notch).max(floor) as usize;
        let max_shift = period - 1;
        // Reachable gap window: clamped near the previous gap and to the field.
        let lo = self.prev_gap.saturating_sub(max_shift);
        let hi = (self.prev_gap + max_shift).min(GW - gw);
        let span = hi - lo + 1;
        let gs = lo + self.rng.below(span);
        let gem_col = if self.rng.chance(1, 3) {
            Some(gs + self.rng.below(gw))
        } else {
            None
        };
        self.walls.push(Wall {
            y: 0,
            gap_start: gs,
            gap_w: gw,
            gem_col,
            gem_taken: false,
            scored: false,
        });
        self.prev_gap = gs;
        self.spawn_timer = period;
    }
}

impl MiniGame for GapDive {
    fn input(&mut self, i: Input) {
        match i {
            Input::Left => self.move_left(),
            Input::Right => self.move_right(),
            _ => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = GapDive::new(self.character, seed, self.opts);
    }

    fn score(&self) -> u32 {
        self.score
    }
    fn lives(&self) -> Option<u32> {
        None // single-mistake survival run
    }
    fn is_over(&self) -> bool {
        self.over
    }
    fn controls(&self) -> &'static str {
        "← → thread the gap  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("walls cleared {}", self.score / 10)
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.score / 180 + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();

        for w in &mut self.walls {
            w.y += 1;
        }

        // Resolve any wall that just reached the glider's row.
        for i in 0..self.walls.len() {
            let w = &self.walls[i];
            if w.scored || w.y < PLAYER_ROW {
                continue;
            }
            let fits = self.px >= w.gap_start && self.px + PLAYER_W <= w.gap_start + w.gap_w;
            self.walls[i].scored = true;
            if !fits {
                self.over = true;
                return;
            }
            self.score += 10;
            let (gem_col, taken) = (self.walls[i].gem_col, self.walls[i].gem_taken);
            if let Some(g) = gem_col {
                if !taken && g >= self.px && g < self.px + PLAYER_W {
                    self.score += 30;
                    self.walls[i].gem_taken = true;
                }
            }
        }

        self.walls.retain(|w| w.y < GH);

        self.spawn_timer = self.spawn_timer.saturating_sub(1);
        if self.spawn_timer == 0 {
            self.spawn(level);
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        // Decorative ground.
        for c in cells[GH - 1].iter_mut() {
            *c = (GROUND, SCENERY);
        }

        // Walls, then gems sitting in their gaps.
        for w in &self.walls {
            if w.y >= GH {
                continue;
            }
            for (x, cell) in cells[w.y].iter_mut().enumerate() {
                if x < w.gap_start || x >= w.gap_start + w.gap_w {
                    *cell = (WALL, colors.bad);
                }
            }
            if let Some(g) = w.gem_col {
                if !w.gem_taken && g < GW {
                    cells[w.y][g] = (GEM, colors.gem);
                }
            }
        }

        // The glider, drawn on top in the character's color.
        let sprite = ['<', '^', '>'];
        let pc = self.character.body_color();
        for (i, ch) in sprite.iter().enumerate() {
            if self.px + i < GW {
                cells[PLAYER_ROW][self.px + i] = (*ch, pc);
            }
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

    #[test]
    fn threading_the_gap_scores_and_survives() {
        let mut g = GapDive::new(Character::Blob, 42, opts());
        g.walls = vec![Wall {
            y: PLAYER_ROW - 1,
            gap_start: g.px,
            gap_w: PLAYER_W,
            gem_col: None,
            gem_taken: false,
            scored: false,
        }];
        g.spawn_timer = 999; // don't spawn during the test
        g.step();
        assert!(!g.is_over());
        assert_eq!(g.score, 10);
    }

    #[test]
    fn hitting_the_wall_ends_the_run() {
        let mut g = GapDive::new(Character::Fox, 7, opts());
        // A gap far to the right; glider on the left is doomed.
        g.walls = vec![Wall {
            y: PLAYER_ROW - 1,
            gap_start: GW - 4,
            gap_w: 4,
            gem_col: None,
            gem_taken: false,
            scored: false,
        }];
        g.px = 0;
        g.spawn_timer = 999;
        g.step();
        assert!(g.is_over());
    }

    #[test]
    fn grabbing_the_gem_adds_bonus() {
        let mut g = GapDive::new(Character::Cat, 3, opts());
        g.walls = vec![Wall {
            y: PLAYER_ROW - 1,
            gap_start: g.px,
            gap_w: PLAYER_W,
            gem_col: Some(g.px + 1),
            gem_taken: false,
            scored: false,
        }];
        g.spawn_timer = 999;
        g.step();
        assert_eq!(g.score, 40); // 10 wall + 30 gem
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = GapDive::new(Character::Robot, 9, opts());
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = GapDive::new(Character::Ghost, 0xF00D, opts());
        for i in 0..4000 {
            if i % 2 == 0 {
                g.move_right();
            } else {
                g.move_left();
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
            assert!(g.px + PLAYER_W <= GW);
        }
    }
}
