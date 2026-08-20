//! "Coil" — a snake with a temper. Steer your growing trail around a small arena
//! eating food (`+`, steady points) and the occasional timed gem (`*`, a fat
//! bonus that vanishes if you dawdle). Every few bites the arena sprouts a rock
//! (`#`) and the pace quickens. Bite a wall, a rock, or your own tail and it ends.
//!
//! One `step()` = one movement tick.

use std::collections::VecDeque;

use super::{coalesce, Input, ItemColors, MiniGame};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;

const FOOD: char = '+';
const GEM: char = '*';
const ROCK: char = '#';
const BODY: char = 'o';
const HEAD: char = 'O';

/// Rocks stop accumulating here so a long game stays survivable.
const ROCK_CAP: usize = 20;
/// How long a gem lingers before it flickers out.
const GEM_TTL: u32 = 40;

/// Direction as a cardinal index: 0=Up, 1=Right, 2=Down, 3=Left.
fn delta(dir: u8) -> (i32, i32) {
    match dir & 3 {
        0 => (0, -1),
        1 => (1, 0),
        2 => (0, 1),
        _ => (-1, 0),
    }
}

fn opposite(a: u8, b: u8) -> bool {
    (a & 3).abs_diff(b & 3) == 2
}

pub struct Coil {
    character: Character,
    body: VecDeque<(usize, usize)>, // front = head
    dir: u8,
    pending: u8, // dir applied at the next step (buffers a fast double-tap)
    grow_owed: usize,
    food: (usize, usize),
    gem: Option<(usize, usize, u32)>, // x, y, ttl
    rocks: Vec<(usize, usize)>,
    eaten: u32,
    score: u32,
    over: bool,
    rng: Rng,
}

impl Coil {
    pub fn new(character: Character, seed: u64) -> Coil {
        let (cx, cy) = (GW / 2, GH / 2);
        let mut body = VecDeque::new();
        body.push_back((cx, cy));
        body.push_back((cx - 1, cy));
        body.push_back((cx - 2, cy));
        let mut g = Coil {
            character,
            body,
            dir: 1,
            pending: 1,
            grow_owed: 0,
            food: (0, 0),
            gem: None,
            rocks: Vec::new(),
            eaten: 0,
            score: 0,
            over: false,
            rng: Rng::new(seed),
        };
        // Seed the first food off the RNG we just built.
        g.food = g.free_cell().unwrap_or((cx + 3, cy));
        g
    }

    /// Is `(x, y)` free of the snake, the rocks, and the current food/gem?
    fn occupied(&self, x: usize, y: usize) -> bool {
        self.body.contains(&(x, y))
            || self.rocks.contains(&(x, y))
            || self.food == (x, y)
            || matches!(self.gem, Some((gx, gy, _)) if (gx, gy) == (x, y))
    }

    /// Find a random empty cell, or `None` if the arena is nearly full.
    fn free_cell(&mut self) -> Option<(usize, usize)> {
        for _ in 0..256 {
            let x = self.rng.below(GW);
            let y = self.rng.below(GH);
            if !self.occupied(x, y) {
                return Some((x, y));
            }
        }
        None
    }

    fn head(&self) -> (usize, usize) {
        *self.body.front().unwrap()
    }

    fn set_dir(&mut self, d: u8) {
        // No 180° reversals into your own neck.
        if !(self.body.len() > 1 && opposite(d, self.dir)) {
            self.pending = d;
        }
    }

    /// Drop a rock somewhere clear of the head's neighborhood (Chebyshev > 2).
    fn add_rock(&mut self) {
        if self.rocks.len() >= ROCK_CAP {
            return;
        }
        let (hx, hy) = self.head();
        for _ in 0..64 {
            let x = self.rng.below(GW);
            let y = self.rng.below(GH);
            let near = (x.abs_diff(hx)).max(y.abs_diff(hy)) <= 2;
            if !near && !self.occupied(x, y) {
                self.rocks.push((x, y));
                return;
            }
        }
    }
}

impl MiniGame for Coil {
    fn input(&mut self, i: Input) {
        match i {
            Input::Up => self.set_dir(0),
            Input::Right => self.set_dir(1),
            Input::Down => self.set_dir(2),
            Input::Left => self.set_dir(3),
            Input::Action => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = Coil::new(self.character, seed);
    }

    fn score(&self) -> u32 {
        self.score
    }
    fn lives(&self) -> Option<u32> {
        None
    }
    fn is_over(&self) -> bool {
        self.over
    }
    fn controls(&self) -> &'static str {
        "↑ ↓ ← → steer  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("length {}   rocks {}", self.body.len(), self.rocks.len())
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.eaten / 5 + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        self.dir = self.pending;
        let (dx, dy) = delta(self.dir);
        let (hx, hy) = self.head();

        // Walls are the arena edges.
        let nx = hx as i32 + dx;
        let ny = hy as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GW as i32 || ny >= GH as i32 {
            self.over = true;
            return;
        }
        let (nx, ny) = (nx as usize, ny as usize);

        // Rocks are instant death.
        if self.rocks.contains(&(nx, ny)) {
            self.over = true;
            return;
        }

        // Self-collision — but the tail cell frees up this tick unless we grow.
        let tail = *self.body.back().unwrap();
        let hits_self = self
            .body
            .iter()
            .any(|&c| c == (nx, ny) && !(c == tail && self.grow_owed == 0));
        if hits_self {
            self.over = true;
            return;
        }

        self.body.push_front((nx, ny));

        let level = self.level();
        if (nx, ny) == self.food {
            self.score += 10 + level * 2;
            self.eaten += 1;
            self.grow_owed += 2;
            // Every fifth bite hardens the arena with a fresh rock.
            if self.eaten % 5 == 0 {
                self.add_rock();
            }
            // Sometimes a timed gem appears alongside the next food.
            if self.gem.is_none() && self.rng.chance(1, 3) {
                if let Some((gx, gy)) = self.free_cell() {
                    self.gem = Some((gx, gy, GEM_TTL));
                }
            }
            self.food = self.free_cell().unwrap_or((nx, ny));
        } else if matches!(self.gem, Some((gx, gy, _)) if (gx, gy) == (nx, ny)) {
            self.score += 50 + level * 5;
            self.eaten += 1;
            self.grow_owed += 1;
            self.gem = None;
        }

        if self.grow_owed > 0 {
            self.grow_owed -= 1;
        } else {
            self.body.pop_back();
        }

        // Age the gem out.
        if let Some((gx, gy, ttl)) = self.gem {
            if ttl <= 1 {
                self.gem = None;
            } else {
                self.gem = Some((gx, gy, ttl - 1));
            }
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        for &(x, y) in &self.rocks {
            cells[y][x] = (ROCK, colors.bad);
        }
        cells[self.food.1][self.food.0] = (FOOD, colors.good);
        if let Some((gx, gy, _)) = self.gem {
            cells[gy][gx] = (GEM, colors.gem);
        }

        let pc = self.character.body_color();
        for (i, &(x, y)) in self.body.iter().enumerate() {
            let ch = if i == 0 { HEAD } else { BODY };
            cells[y][x] = (ch, pc);
        }

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
    fn eating_food_grows_and_scores() {
        let mut g = Coil::new(Character::Blob, 1);
        let (hx, hy) = g.head();
        // Put food directly ahead (moving right).
        g.food = (hx + 1, hy);
        let before = g.body.len();
        g.step();
        assert_eq!(g.eaten, 1);
        assert!(g.score >= 12);
        // grow_owed was +2, spent 1 this tick → net +1 cell now, +1 pending.
        assert!(g.body.len() > before);
    }

    #[test]
    fn hitting_a_wall_ends_it() {
        let mut g = Coil::new(Character::Fox, 2);
        // Drive straight into the right wall.
        for _ in 0..GW {
            g.step();
            if g.is_over() {
                break;
            }
        }
        assert!(g.is_over());
    }

    #[test]
    fn cannot_reverse_into_neck() {
        let mut g = Coil::new(Character::Cat, 3);
        // Facing right; trying to go left must be ignored.
        g.input(Input::Left);
        g.step();
        assert!(!g.is_over(), "a reversal should be dropped, not fatal");
    }

    #[test]
    fn hitting_a_rock_ends_it() {
        let mut g = Coil::new(Character::Robot, 4);
        let (hx, hy) = g.head();
        g.rocks = vec![(hx + 1, hy)];
        g.food = (0, 0); // keep food out of the way
        g.step();
        assert!(g.is_over());
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = Coil::new(Character::Ghost, 5);
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = Coil::new(Character::Dragon, 0xC01D);
        let turns = [Input::Up, Input::Right, Input::Down, Input::Left];
        for i in 0..5000 {
            g.input(turns[(i / 3) % 4]);
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
            for &(x, y) in &g.body {
                assert!(x < GW && y < GH);
            }
        }
    }
}
