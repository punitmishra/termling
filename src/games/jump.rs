//! "Doodle Dash" — an endless runner. Your character sprints in place while
//! cacti (`#`) scroll in from the right along the ground. Tap `Space`/`↑` to
//! hop over them; a mistimed jump ends the dash. You score for distance and earn
//! a bonus for every obstacle you clear, and the ground scrolls faster as you go.
//!
//! One `step()` = one column of scroll.

use super::{coalesce, Input, ItemColors, MiniGame, SCENERY};
use crate::doodle::{Character, Row};
use crate::rng::Rng;
use ratatui::style::Color;

const GW: usize = 22;
const GH: usize = 12;
/// The row the runner and cacti stand on (one above the drawn ground line).
const STAND_ROW: usize = 9;
const GROUND_ROW: usize = 10;
/// The runner's fixed column.
const PLAYER_COL: usize = 3;
/// How many ticks a jump lasts.
const JUMP_TICKS: u32 = 7;

const CACTUS: char = '#';

struct Obstacle {
    x: i32,
    cleared: bool,
}

pub struct Jump {
    character: Character,
    air: u32, // 0 = grounded, else ticks remaining in the hop
    obstacles: Vec<Obstacle>,
    spawn_timer: u32,
    score: u32,
    over: bool,
    rng: Rng,
}

impl Jump {
    pub fn new(character: Character, seed: u64) -> Jump {
        Jump {
            character,
            air: 0,
            obstacles: Vec::new(),
            spawn_timer: 8,
            score: 0,
            over: false,
            rng: Rng::new(seed),
        }
    }

    /// Height above the ground for the current hop phase (a little triangle).
    fn height(&self) -> i32 {
        if self.air == 0 {
            return 0;
        }
        let up = (JUMP_TICKS - self.air) as i32;
        up.min(JUMP_TICKS as i32 - 1 - up) + 1
    }

    fn jump(&mut self) {
        if !self.over && self.air == 0 {
            self.air = JUMP_TICKS;
        }
    }

    fn spawn(&mut self, level: u32) {
        self.obstacles.push(Obstacle {
            x: (GW - 1) as i32,
            cleared: false,
        });
        // Space obstacles so a single hop can always recover; tighten with level.
        let gap = (13u32.saturating_sub(level)).max(JUMP_TICKS + 1);
        self.spawn_timer = gap + self.rng.below(4) as u32;
    }
}

impl MiniGame for Jump {
    fn input(&mut self, i: Input) {
        if matches!(i, Input::Up | Input::Action) {
            self.jump();
        }
    }

    fn restart(&mut self) {
        let seed = self.rng.next_u64();
        *self = Jump::new(self.character, seed);
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
        "space / ↑ jump  ·  esc back"
    }
    fn hud(&self) -> String {
        format!("distance {}", self.score / 10)
    }
    fn character_name(&self) -> &'static str {
        self.character.name()
    }

    fn level(&self) -> u32 {
        self.score / 150 + 1
    }

    fn step(&mut self) {
        if self.over {
            return;
        }
        let level = self.level();
        self.score += 1;
        if self.air > 0 {
            self.air -= 1;
        }

        for o in &mut self.obstacles {
            o.x -= 1;
        }
        self.obstacles.retain(|o| o.x >= 0);

        let grounded = self.height() == 0;
        for o in &mut self.obstacles {
            if o.x == PLAYER_COL as i32 {
                if grounded {
                    self.over = true;
                    return;
                } else if !o.cleared {
                    o.cleared = true;
                    self.score += 10;
                }
            }
        }

        self.spawn_timer = self.spawn_timer.saturating_sub(1);
        if self.spawn_timer == 0 {
            self.spawn(level);
        }
    }

    fn render(&self, colors: ItemColors) -> Vec<Row> {
        let mut cells = vec![vec![(' ', Color::Reset); GW]; GH];

        // Ground line.
        for c in cells[GROUND_ROW].iter_mut() {
            *c = ('═', SCENERY);
        }

        // Cacti.
        for o in &self.obstacles {
            if (0..GW as i32).contains(&o.x) {
                cells[STAND_ROW][o.x as usize] = (CACTUS, colors.bad);
            }
        }

        // The runner, lifted by the current hop height.
        let row = STAND_ROW - self.height() as usize;
        cells[row][PLAYER_COL] = ('O', self.character.body_color());

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
    fn running_into_a_cactus_ends_it() {
        let mut g = Jump::new(Character::Blob, 1);
        g.obstacles = vec![Obstacle {
            x: PLAYER_COL as i32 + 1,
            cleared: false,
        }];
        g.spawn_timer = 999;
        g.step(); // cactus moves onto the player column while grounded
        assert!(g.is_over());
    }

    #[test]
    fn jumping_clears_a_cactus_for_a_bonus() {
        let mut g = Jump::new(Character::Fox, 2);
        g.obstacles = vec![Obstacle {
            x: PLAYER_COL as i32 + 1,
            cleared: false,
        }];
        g.spawn_timer = 999;
        g.jump();
        let before = g.score;
        g.step();
        assert!(!g.is_over());
        assert_eq!(g.score, before + 1 + 10); // step + clear bonus
    }

    #[test]
    fn a_hop_returns_to_the_ground() {
        let mut g = Jump::new(Character::Cat, 3);
        g.jump();
        assert!(g.height() > 0);
        g.spawn_timer = 999;
        for _ in 0..JUMP_TICKS {
            g.step();
        }
        assert_eq!(g.height(), 0);
    }

    #[test]
    fn render_has_stable_dimensions() {
        let g = Jump::new(Character::Robot, 4);
        let rows = g.render(colors());
        assert_eq!(rows.len(), GH);
        for row in rows {
            let width: usize = row.iter().map(|s| s.text.chars().count()).sum();
            assert_eq!(width, GW);
        }
    }

    #[test]
    fn many_steps_never_panic() {
        let mut g = Jump::new(Character::Dragon, 0x0FF);
        for i in 0..5000 {
            if i % 9 == 0 {
                g.jump();
            }
            g.step();
            let _ = g.render(colors());
            if g.is_over() {
                g.restart();
            }
        }
    }
}
