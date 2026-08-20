//! "Muncher" — a pac-style pellet chase. Steer your muncher (`C`) through the
//! corridors gobbling pellets (`·`); grab a power dot (`O`) and the ghost (`M`)
//! turns edible (`w`) for a short while — catch it for a bonus. Clear the board
//! and it refills, a little faster each time. Touch the ghost while it's hunting
//! and you lose a life.
//!
//! One `step()` = one tick: the muncher drifts in its current heading and the
//! ghost takes a step toward you (or away, while it's frightened).

use super::{coalesce, Input, ItemColors, MiniGame, SCENERY};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;
/// Where the muncher and ghost begin each life (col, row).
const PLAYER_START: (usize, usize) = (1, 1);
const GHOST_START: (usize, usize) = (GW - 3, GH - 2);
const START_LIVES: u32 = 3;
/// How long a power dot keeps the ghost edible.
const FRIGHT_TICKS: u32 = 30;
/// The four power dots (col, row) — all on open floor.
const POWER: [(usize, usize); 4] = [(GW - 2, 1), (1, GH - 2), (GW - 2, GH - 2), (10, 5)];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tile {
    Wall,
    Empty,
    Pellet,
    Power,
}

pub struct Muncher {
    character: Character,
    grid: Vec<Vec<Tile>>,
    px: usize,
    py: usize,
    dir: Option<usize>, // 0 up, 1 right, 2 down, 3 left
    gx: usize,
    gy: usize,
    frightened: u32,
    score: u32,
    lives: u32,
    boards: u32,
    tick: u64,
    over: bool,
    rng: Rng,
}

impl Muncher {
    pub fn new(character: Character, seed: u64) -> Muncher {
        let grid = Muncher::build_grid();
        let mut g = Muncher {
            character,
            grid,
            px: PLAYER_START.0,
            py: PLAYER_START.1,
            dir: None,
            gx: GHOST_START.0,
            gy: GHOST_START.1,
            frightened: 0,
            score: 0,
            lives: START_LIVES,
            boards: 0,
            tick: 0,
            over: false,
            rng: Rng::new(seed),
        };
        g.grid[g.py][g.px] = Tile::Empty;
        g.grid[g.gy][g.gx] = Tile::Empty;
        g
    }

    /// A fully-connected maze: solid border, single-cell pillars on the interior
    /// even/even lattice (so every corridor stays reachable), pellets elsewhere.
    fn build_grid() -> Vec<Vec<Tile>> {
        let mut grid = vec![vec![Tile::Pellet; GW]; GH];
        for (r, row) in grid.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                let border = r == 0 || r == GH - 1 || c == 0 || c == GW - 1;
                let pillar = (2..=GH - 3).contains(&r)
                    && (2..=GW - 3).contains(&c)
                    && r % 2 == 0
                    && c % 2 == 0;
                if border || pillar {
                    *cell = Tile::Wall;
                }
            }
        }
        for &(c, r) in &POWER {
            grid[r][c] = Tile::Power;
        }
        grid
    }

    fn delta(dir: usize) -> (i32, i32) {
        match dir {
            0 => (0, -1),
            1 => (1, 0),
            2 => (0, 1),
            _ => (-1, 0),
        }
    }

    fn is_wall(&self, c: i32, r: i32) -> bool {
        if c < 0 || r < 0 || c >= GW as i32 || r >= GH as i32 {
            return true;
        }
        self.grid[r as usize][c as usize] == Tile::Wall
    }

    fn ghost_period(&self) -> u64 {
        let base = 3u64.saturating_sub(self.level() as u64).max(1);
        // Frightened ghosts dawdle, giving you a window to catch them.
        if self.frightened > 0 {
            base + 1
        } else {
            base
        }
    }

    /// The ghost's next cell: the open neighbour that minimises distance to the
    /// muncher — or maximises it while frightened. Ties break in a fixed order.
    fn ghost_next(&self) -> (usize, usize) {
        let flee = self.frightened > 0;
        let mut best = (self.gx, self.gy);
        let mut best_dist = if flee { -1 } else { i32::MAX };
        for dir in 0..4 {
            let (dc, dr) = Muncher::delta(dir);
            let nc = self.gx as i32 + dc;
            let nr = self.gy as i32 + dr;
            if self.is_wall(nc, nr) {
                continue;
            }
            let dist = (self.px as i32 - nc).abs() + (self.py as i32 - nr).abs();
            let better = if flee {
                dist > best_dist
            } else {
                dist < best_dist
            };
            if better {
                best_dist = dist;
                best = (nc as usize, nr as usize);
            }
        }
        best
    }

    /// Slide the muncher one cell in `dir`, eating whatever it lands on. A wall
    /// simply stops it (the heading is kept, so it resumes when the way clears).
    fn move_player(&mut self, dir: usize) {
        let (dc, dr) = Muncher::delta(dir);
        let nc = self.px as i32 + dc;
        let nr = self.py as i32 + dr;
        if self.is_wall(nc, nr) {
            return;
        }
        self.px = nc as usize;
        self.py = nr as usize;
        match self.grid[self.py][self.px] {
            Tile::Pellet => {
                self.score += 5;
                self.grid[self.py][self.px] = Tile::Empty;
            }
            Tile::Power => {
                self.score += 25;
                self.frightened = FRIGHT_TICKS;
                self.grid[self.py][self.px] = Tile::Empty;
            }
            _ => {}
        }
    }

    fn resolve_collision(&mut self) {
        if self.px != self.gx || self.py != self.gy {
            return;
        }
        if self.frightened > 0 {
            // Eat the ghost; it respawns at its den and the fright clock rolls on.
            self.score += 50;
            self.gx = GHOST_START.0;
            self.gy = GHOST_START.1;
        } else {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.over = true;
            } else {
                self.px = PLAYER_START.0;
                self.py = PLAYER_START.1;
                self.gx = GHOST_START.0;
                self.gy = GHOST_START.1;
                self.dir = None;
                self.frightened = 0;
            }
        }
    }

    fn pellets_left(&self) -> usize {
        self.grid
            .iter()
            .flatten()
            .filter(|t| matches!(t, Tile::Pellet | Tile::Power))
            .count()
    }

    /// Refill the board for another lap, keeping score and current positions.
    fn refill(&mut self) {
        self.boards += 1;
        self.grid = Muncher::build_grid();
        self.grid[self.py][self.px] = Tile::Empty;
        self.grid[self.gy][self.gx] = Tile::Empty;
    }
}

impl MiniGame for Muncher {
    fn input(&mut self, i: Input) {
        match i {
            Input::Up => self.dir = Some(0),
            Input::Right => self.dir = Some(1),
            Input::Down => self.dir = Some(2),
            Input::Left => self.dir = Some(3),
            Input::Action => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = Muncher::new(self.character, seed);
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
        "← ↑ ↓ → steer  ·  esc back"
    }
    fn hud(&self) -> String {
        if self.frightened > 0 {
            format!("pellets {}  ·  ghost scared!", self.pellets_left())
        } else {
            format!(
                "pellets {}  ·  board {}",
                self.pellets_left(),
                self.boards + 1
            )
        }
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.boards + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        self.tick = self.tick.wrapping_add(1);
        if self.frightened > 0 {
            self.frightened -= 1;
        }

        if let Some(d) = self.dir {
            self.move_player(d);
        }
        self.resolve_collision();
        if self.over {
            return;
        }

        if self.tick % self.ghost_period() == 0 {
            let (nx, ny) = self.ghost_next();
            self.gx = nx;
            self.gy = ny;
        }
        self.resolve_collision();

        if self.pellets_left() == 0 {
            self.refill();
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        for (r, row) in self.grid.iter().enumerate() {
            for (c, tile) in row.iter().enumerate() {
                cells[r][c] = match tile {
                    Tile::Wall => ('#', SCENERY),
                    Tile::Empty => (' ', Color::Reset),
                    Tile::Pellet => ('·', colors.good),
                    Tile::Power => ('O', colors.gem),
                };
            }
        }

        // Ghost, then muncher on top.
        let (gch, gcol) = if self.frightened > 0 {
            ('w', colors.gem)
        } else {
            ('M', colors.bad)
        };
        cells[self.gy][self.gx] = (gch, gcol);
        cells[self.py][self.px] = ('C', self.character.body_color());

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
    fn eating_a_pellet_scores() {
        let mut g = Muncher::new(Character::Blob, 1);
        // (1,1) is clear; (2,1) to its right is a pellet on the open border row.
        assert_eq!(g.grid[1][2], Tile::Pellet);
        g.dir = Some(1); // right
        g.step();
        assert_eq!((g.px, g.py), (2, 1));
        assert_eq!(g.score, 5);
        assert_eq!(g.grid[1][2], Tile::Empty);
    }

    #[test]
    fn a_power_dot_frightens_the_ghost() {
        let mut g = Muncher::new(Character::Cat, 2);
        // Drop the muncher next to a power dot and step onto it.
        g.px = POWER[0].0 - 1;
        g.py = POWER[0].1;
        g.dir = Some(1);
        g.step();
        assert!(g.frightened > 0);
        assert!(g.score >= 25);
    }

    #[test]
    fn the_ghost_catching_you_costs_a_life() {
        let mut g = Muncher::new(Character::Fox, 3);
        // Park the ghost right of the muncher, no fright, and walk into it. The
        // ghost won't move this tick (period 2, tick becomes 1).
        g.px = 5;
        g.py = 5;
        g.gx = 6;
        g.gy = 5;
        g.frightened = 0;
        g.dir = Some(1);
        g.step();
        assert_eq!(g.lives, START_LIVES - 1);
        // Positions reset to the starts after losing a life.
        assert_eq!((g.px, g.py), PLAYER_START);
    }

    #[test]
    fn eating_a_scared_ghost_scores() {
        let mut g = Muncher::new(Character::Ghost, 4);
        g.px = 5;
        g.py = 5;
        g.gx = 6;
        g.gy = 5;
        g.frightened = FRIGHT_TICKS;
        g.dir = Some(1);
        g.step();
        assert!(g.score >= 50);
        assert_eq!(g.lives, START_LIVES);
        assert_eq!((g.gx, g.gy), GHOST_START);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = Muncher::new(Character::Robot, 5);
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = Muncher::new(Character::Dragon, 0x51A2);
        for i in 0..5000 {
            match i % 4 {
                0 => g.input(Input::Left),
                1 => g.input(Input::Up),
                2 => g.input(Input::Right),
                _ => g.input(Input::Down),
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
        }
    }
}
