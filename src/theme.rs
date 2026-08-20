//! Themes — a small palette for the shell's *chrome* (borders, HUD, XP bar) and
//! the mini-game's falling items. Characters keep their own colors (that's their
//! identity); a theme just sets the surrounding tone so it suits your terminal.
//!
//! Lightweight by design: a `Theme` is six colors and a name, and the whole set
//! is a `const` array — switching is just an index.

use ratatui::style::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    /// Titles, borders, the "level" line.
    pub accent: Color,
    /// Secondary/muted text.
    pub dim: Color,
    /// XP-bar fill.
    pub bar: Color,
    /// Game: good drops, gems, bombs.
    pub good: Color,
    pub gem: Color,
    pub bad: Color,
}

/// The switchable set. Add one here and it joins the `t`-key rotation.
pub const THEMES: &[Theme] = &[
    Theme {
        name: "aurora",
        accent: Color::Rgb(150, 220, 190),
        dim: Color::Rgb(120, 130, 140),
        bar: Color::Rgb(150, 220, 190),
        good: Color::Rgb(255, 220, 120),
        gem: Color::Rgb(120, 220, 255),
        bad: Color::Rgb(255, 95, 95),
    },
    Theme {
        name: "neon",
        accent: Color::Rgb(80, 240, 220),
        dim: Color::Rgb(110, 100, 150),
        bar: Color::Rgb(255, 80, 200),
        good: Color::Rgb(120, 255, 140),
        gem: Color::Rgb(120, 200, 255),
        bad: Color::Rgb(255, 60, 120),
    },
    Theme {
        name: "sunset",
        accent: Color::Rgb(255, 170, 90),
        dim: Color::Rgb(150, 120, 110),
        bar: Color::Rgb(255, 140, 80),
        good: Color::Rgb(255, 215, 130),
        gem: Color::Rgb(255, 130, 170),
        bad: Color::Rgb(210, 60, 70),
    },
    Theme {
        name: "matrix",
        accent: Color::Rgb(90, 220, 120),
        dim: Color::Rgb(60, 120, 70),
        bar: Color::Rgb(120, 240, 140),
        good: Color::Rgb(180, 255, 180),
        gem: Color::Rgb(120, 255, 200),
        bad: Color::Rgb(120, 160, 120),
    },
    Theme {
        name: "mono",
        accent: Color::Rgb(220, 220, 225),
        dim: Color::Rgb(120, 120, 128),
        bar: Color::Rgb(200, 200, 205),
        good: Color::Rgb(235, 235, 240),
        gem: Color::Rgb(255, 255, 255),
        bad: Color::Rgb(150, 150, 155),
    },
    Theme {
        name: "ocean",
        accent: Color::Rgb(80, 200, 230),
        dim: Color::Rgb(90, 110, 130),
        bar: Color::Rgb(60, 180, 220),
        good: Color::Rgb(150, 230, 200),
        gem: Color::Rgb(255, 230, 150),
        bad: Color::Rgb(255, 110, 120),
    },
    Theme {
        name: "ember",
        accent: Color::Rgb(255, 140, 70),
        dim: Color::Rgb(140, 110, 100),
        bar: Color::Rgb(255, 110, 60),
        good: Color::Rgb(255, 200, 120),
        gem: Color::Rgb(255, 240, 180),
        bad: Color::Rgb(200, 50, 50),
    },
    Theme {
        name: "sakura",
        accent: Color::Rgb(255, 160, 200),
        dim: Color::Rgb(170, 140, 160),
        bar: Color::Rgb(255, 150, 190),
        good: Color::Rgb(200, 240, 180),
        gem: Color::Rgb(255, 240, 200),
        bad: Color::Rgb(220, 90, 120),
    },
    Theme {
        name: "coffee",
        accent: Color::Rgb(200, 160, 120),
        dim: Color::Rgb(130, 110, 95),
        bar: Color::Rgb(180, 130, 90),
        good: Color::Rgb(210, 190, 150),
        gem: Color::Rgb(240, 220, 170),
        bad: Color::Rgb(170, 90, 70),
    },
    Theme {
        name: "vaporwave",
        accent: Color::Rgb(255, 120, 220),
        dim: Color::Rgb(140, 120, 180),
        bar: Color::Rgb(120, 220, 255),
        good: Color::Rgb(150, 255, 220),
        gem: Color::Rgb(255, 240, 140),
        bad: Color::Rgb(255, 90, 160),
    },
    Theme {
        name: "forest",
        accent: Color::Rgb(120, 200, 140),
        dim: Color::Rgb(100, 120, 100),
        bar: Color::Rgb(90, 180, 110),
        good: Color::Rgb(200, 230, 150),
        gem: Color::Rgb(240, 220, 150),
        bad: Color::Rgb(200, 120, 80),
    },
];
