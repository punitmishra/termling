//! "Brick Bounce" — a compact breakout. Slide your paddle (`▔▔▔▔`) along the
//! floor and keep the ball (`o`) in play; every brick it clears scores, and the
//! rare golden brick pays extra. Clear the wall and a fresh one drops in, faster
//! each time. Miss the ball and you lose a life.
//!
//! One `step()` = one ball move (one cell on each axis), reflecting off the side
//! walls, the ceiling, the bricks, and your paddle.

use super::{coalesce, Input, ItemColors, MiniGame};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;
const PADDLE_ROW: usize = GH - 1;
const PADDLE_W: usize = 5;
/// The brick wall occupies these rows.
const BRICK_TOP: usize = 2;
const BRICK_ROWS: usize = 3;
const START_LIVES: u32 = 3;

pub struct Bounce {
    character: Character,
    px: usize,            // paddle left edge
    bx: i32,              // ball column
    by: i32,              // ball row
    vx: i32,              // ±1
    vy: i32,              // ±1
    bricks: Vec<Vec<u8>>, // [BRICK_ROWS][GW]: 0 empty, 1 normal, 2 golden
    score: u32,
    lives: u32,
    boards: u32,
    over: bool,
    rng: Rng,
}

impl Bounce {
    pub fn new(character: Character, seed: u64) -> Bounce {
        let mut rng = Rng::new(seed);
        let bricks = Bounce::build_wall(&mut rng);
        let px = (GW - PADDLE_W) / 2;
        Bounce {
            character,
            px,
            bx: (GW / 2) as i32,
            by: (PADDLE_ROW - 1) as i32,
            vx: 1,
            vy: -1,
            bricks,
            score: 0,
            lives: START_LIVES,
            boards: 0,
            over: false,
            rng,
        }
    }

    /// A fresh brick wall — mostly normal bricks with a scattering of golden ones.
    fn build_wall(rng: &mut Rng) -> Vec<Vec<u8>> {
        (0..BRICK_ROWS)
            .map(|_| {
                (0..GW)
                    .map(|_| if rng.chance(1, 8) { 2 } else { 1 })
                    .collect()
            })
            .collect()
    }

    fn paddle_hit(&self, col: i32) -> bool {
        col >= self.px as i32 && col < (self.px + PADDLE_W) as i32
    }

    /// Knock out a brick at the ball's cell if one is there; return whether it hit.
    fn hit_brick(&mut self) -> bool {
        if self.by < BRICK_TOP as i32 || self.by >= (BRICK_TOP + BRICK_ROWS) as i32 {
            return false;
        }
        if self.bx < 0 || self.bx >= GW as i32 {
            return false;
        }
        let r = (self.by - BRICK_TOP as i32) as usize;
        let c = self.bx as usize;
        match self.bricks[r][c] {
            0 => false,
            kind => {
                self.score += if kind == 2 { 30 } else { 10 };
                self.bricks[r][c] = 0;
                true
            }
        }
    }

    fn bricks_left(&self) -> usize {
        self.bricks.iter().flatten().filter(|&&b| b != 0).count()
    }

    fn reset_ball(&mut self) {
        self.bx = (self.px + PADDLE_W / 2) as i32;
        self.by = (PADDLE_ROW - 1) as i32;
        self.vx = if self.rng.chance(1, 2) { 1 } else { -1 };
        self.vy = -1;
    }

    fn refill(&mut self) {
        self.boards += 1;
        self.bricks = Bounce::build_wall(&mut self.rng);
        self.reset_ball();
    }
}

impl MiniGame for Bounce {
    fn input(&mut self, i: Input) {
        match i {
            Input::Left if self.px > 0 => self.px -= 1,
            Input::Right if self.px + PADDLE_W < GW => self.px += 1,
            _ => {}
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = Bounce::new(self.character, seed);
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
        "← → paddle  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("bricks {}  ·  wall {}", self.bricks_left(), self.boards + 1)
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

        // Horizontal move, reflecting off the side walls.
        let mut nbx = self.bx + self.vx;
        if nbx < 0 || nbx >= GW as i32 {
            self.vx = -self.vx;
            nbx = self.bx + self.vx;
        }
        self.bx = nbx;

        // Vertical move, reflecting off the ceiling.
        let mut nby = self.by + self.vy;
        if nby < 0 {
            self.vy = 1;
            nby = self.by + self.vy;
        }
        self.by = nby;

        // Bricks bounce the ball straight back.
        if self.hit_brick() {
            self.vy = -self.vy;
        }

        // The paddle line: bounce if caught, otherwise it's a miss.
        if self.by == PADDLE_ROW as i32 {
            if self.paddle_hit(self.bx) {
                self.vy = -1;
                self.by = (PADDLE_ROW - 1) as i32;
                // Steer off the paddle: left half sends it left, right half right.
                let half = self.px as i32 + (PADDLE_W as i32) / 2;
                self.vx = if self.bx < half { -1 } else { 1 };
            } else {
                self.lives = self.lives.saturating_sub(1);
                if self.lives == 0 {
                    self.over = true;
                    return;
                }
                self.reset_ball();
            }
        }

        if self.bricks_left() == 0 {
            self.refill();
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        // Bricks.
        for (r, row) in self.bricks.iter().enumerate() {
            for (c, &kind) in row.iter().enumerate() {
                if kind != 0 {
                    let col = if kind == 2 { colors.gem } else { colors.good };
                    cells[BRICK_TOP + r][c] = ('▒', col);
                }
            }
        }

        // Paddle.
        for c in self.px..self.px + PADDLE_W {
            cells[PADDLE_ROW][c] = ('▔', self.character.body_color());
        }

        // Ball.
        if self.by >= 0 && (self.by as usize) < GH && self.bx >= 0 && (self.bx as usize) < GW {
            cells[self.by as usize][self.bx as usize] = ('o', colors.bad);
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
    fn clearing_a_brick_scores_and_bounces() {
        let mut g = Bounce::new(Character::Blob, 1);
        // Aim the ball to rise into a known brick cell.
        g.bricks = vec![vec![0u8; GW]; BRICK_ROWS];
        g.bricks[0][5] = 1; // maps to grid row BRICK_TOP, col 5
        g.bricks[2][0] = 1; // a spare so the board isn't instantly cleared
        g.bx = 4;
        g.by = BRICK_TOP as i32 + 1;
        g.vx = 1;
        g.vy = -1;
        g.step(); // moves to (5, BRICK_TOP) into the brick
        assert_eq!(g.score, 10);
        assert_eq!(g.bricks[0][5], 0);
        assert_eq!(g.vy, 1); // reflected downward
    }

    #[test]
    fn the_paddle_bounces_the_ball() {
        let mut g = Bounce::new(Character::Cat, 2);
        g.bricks = vec![vec![1u8; GW]; BRICK_ROWS]; // keep the board unfinished
        g.px = 8;
        g.bx = 10; // over the paddle
        g.by = (PADDLE_ROW - 1) as i32;
        g.vx = 1;
        g.vy = 1;
        g.step();
        assert_eq!(g.vy, -1);
        assert_eq!(g.lives, START_LIVES);
    }

    #[test]
    fn missing_the_ball_costs_a_life() {
        let mut g = Bounce::new(Character::Fox, 3);
        g.bricks = vec![vec![1u8; GW]; BRICK_ROWS];
        g.px = 0; // paddle far to the left
        g.bx = GW as i32 - 1; // ball drops on the far right
        g.by = (PADDLE_ROW - 1) as i32;
        g.vx = 1;
        g.vy = 1;
        g.step();
        assert_eq!(g.lives, START_LIVES - 1);
    }

    #[test]
    fn paddle_stays_in_bounds() {
        let mut g = Bounce::new(Character::Ghost, 4);
        for _ in 0..GW * 2 {
            g.input(Input::Left);
        }
        assert_eq!(g.px, 0);
        for _ in 0..GW * 2 {
            g.input(Input::Right);
        }
        assert_eq!(g.px, GW - PADDLE_W);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = Bounce::new(Character::Robot, 5);
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = Bounce::new(Character::Jellyfish, 0x9EE7);
        for i in 0..5000 {
            if i % 3 == 0 {
                g.input(Input::Left);
            } else {
                g.input(Input::Right);
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
        }
    }
}
