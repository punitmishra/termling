//! The doodles: a cast of animated, multi-color ASCII characters.
//!
//! Three ideas are kept orthogonal so they multiply instead of add:
//!
//!   * A [`Character`] is a *silhouette + palette* — the head top, the body
//!     sides, the base, and four accent colors. That's its identity.
//!   * A mood is an *expression* — the eyes, mouth, and the particles drifting
//!     around the creature (see [`expression`]). Any mood renders on any
//!     character, so adding either one lights up every combination for free.
//!   * A [`Scene`] is the *backdrop* — sparse, twinkling glyphs in the sky,
//!     ground, and side margins, chosen to fit the mood (see [`scene_for`]).
//!
//! [`render`] composites them onto a fixed 20×6 canvas as colored [`Seg`]ments —
//! the character painted over its backdrop — which the UI turns into ratatui
//! spans. The active character rotates over time (driven by `App::character_seed`),
//! so the pane cycles through the whole cast as you work.

use crate::mood::Mood;
use ratatui::style::Color;

/// Canvas width. Every row is padded to this so the centered block never jitters.
const W: usize = 20;
/// Width of the face block that sits centered inside the canvas.
const FACE: usize = 10;

/// A run of same-colored text — the atom the UI renders as one span.
pub struct Seg {
    pub text: String,
    pub color: Color,
}

pub type Row = Vec<Seg>;

pub struct Doodle {
    pub rows: Vec<Row>,
    pub name: &'static str,
    pub caption: String,
    pub caption_color: Color,
    pub border_color: Color,
}

// --- characters ------------------------------------------------------------

/// A character's four working colors.
struct Palette {
    base: Color,   // body / outline
    accent: Color, // particles, caption, border
    eye: Color,
    mouth: Color,
}

#[derive(Clone, Copy)]
pub enum Character {
    Blob,
    Cat,
    Robot,
    Ghost,
    Fox,
    // --- the expanded cast ---
    Comet,
    StarSprite,
    Mixtape,
    TubeBoy,
    Jellyfish,
    Dragon,
}

impl Character {
    /// The rotating cast. Add a variant here and it joins the rotation.
    pub const CAST: [Character; 11] = [
        Character::Blob,
        Character::Cat,
        Character::Robot,
        Character::Ghost,
        Character::Fox,
        Character::Comet,
        Character::StarSprite,
        Character::Mixtape,
        Character::TubeBoy,
        Character::Jellyfish,
        Character::Dragon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Character::Blob => "blob",
            Character::Cat => "cat",
            Character::Robot => "robot",
            Character::Ghost => "ghost",
            Character::Fox => "fox",
            Character::Comet => "comet",
            Character::StarSprite => "astra",
            Character::Mixtape => "mixtape",
            Character::TubeBoy => "tubeboy",
            Character::Jellyfish => "jelly",
            Character::Dragon => "draco",
        }
    }

    /// Top of the head — ears, antenna, dome. Centered over the eyes.
    fn top(self) -> &'static str {
        match self {
            Character::Blob => ".------.",
            Character::Cat => "/\\_/\\",
            Character::Robot => ".-|-.",
            Character::Ghost => ".-~~-.",
            Character::Fox => "/\\ /\\",
            Character::Comet => "*=--~~",
            Character::StarSprite => "\\ | /",
            Character::Mixtape => "(o) (o)",
            Character::TubeBoy => "\\o o/",
            Character::Jellyfish => ".:oOo:.",
            Character::Dragon => "^vVVVv^",
        }
    }

    /// The base line the creature sits on.
    fn base(self) -> &'static str {
        match self {
            Character::Blob => "'------'",
            Character::Cat => "'--v--'",
            Character::Robot => "[____]",
            Character::Ghost => "~ ~ ~",
            Character::Fox => "'--w--'",
            Character::Comet => ".*. .*.",
            Character::StarSprite => "/ | \\",
            Character::Mixtape => "'-o--o-'",
            Character::TubeBoy => "o_====_o",
            Character::Jellyfish => "}{}{}{}",
            Character::Dragon => "'wWWWw'",
        }
    }

    /// The left/right body walls framing the eyes and mouth.
    fn sides(self) -> (char, char) {
        match self {
            Character::Robot => ('|', '|'),
            Character::StarSprite => ('*', '*'),
            Character::Mixtape => ('[', ']'),
            Character::TubeBoy => ('{', '}'),
            Character::Dragon => ('{', '}'),
            _ => ('(', ')'),
        }
    }

    /// Body/outline color — used by the mini-game to tint the player sprite.
    pub fn body_color(self) -> Color {
        self.palette().base
    }

    fn palette(self) -> Palette {
        match self {
            Character::Blob => Palette {
                base: Color::Rgb(120, 200, 130),
                accent: Color::Rgb(180, 255, 120),
                eye: Color::Rgb(245, 245, 245),
                mouth: Color::Rgb(150, 220, 150),
            },
            Character::Cat => Palette {
                base: Color::Rgb(240, 150, 70),
                accent: Color::Rgb(255, 150, 190),
                eye: Color::Rgb(255, 230, 120),
                mouth: Color::Rgb(255, 185, 120),
            },
            Character::Robot => Palette {
                base: Color::Rgb(150, 170, 195),
                accent: Color::Rgb(120, 225, 235),
                eye: Color::Rgb(255, 95, 95),
                mouth: Color::Rgb(205, 215, 225),
            },
            Character::Ghost => Palette {
                base: Color::Rgb(200, 190, 240),
                accent: Color::Rgb(150, 200, 255),
                eye: Color::Rgb(250, 250, 255),
                mouth: Color::Rgb(210, 200, 245),
            },
            Character::Fox => Palette {
                base: Color::Rgb(235, 120, 60),
                accent: Color::Rgb(255, 240, 230),
                eye: Color::Rgb(255, 210, 120),
                mouth: Color::Rgb(250, 160, 110),
            },
            Character::Comet => Palette {
                base: Color::Rgb(255, 180, 90),
                accent: Color::Rgb(255, 240, 150),
                eye: Color::Rgb(255, 255, 255),
                mouth: Color::Rgb(255, 140, 80),
            },
            Character::StarSprite => Palette {
                base: Color::Rgb(255, 230, 120),
                accent: Color::Rgb(180, 220, 255),
                eye: Color::Rgb(255, 255, 255),
                mouth: Color::Rgb(255, 210, 120),
            },
            Character::Mixtape => Palette {
                base: Color::Rgb(200, 120, 220),
                accent: Color::Rgb(120, 230, 220),
                eye: Color::Rgb(255, 240, 150),
                mouth: Color::Rgb(230, 180, 240),
            },
            Character::TubeBoy => Palette {
                base: Color::Rgb(130, 180, 200),
                accent: Color::Rgb(120, 255, 180),
                eye: Color::Rgb(120, 255, 180),
                mouth: Color::Rgb(180, 220, 230),
            },
            Character::Jellyfish => Palette {
                base: Color::Rgb(150, 190, 255),
                accent: Color::Rgb(255, 170, 220),
                eye: Color::Rgb(255, 255, 255),
                mouth: Color::Rgb(190, 210, 255),
            },
            Character::Dragon => Palette {
                base: Color::Rgb(120, 200, 120),
                accent: Color::Rgb(255, 150, 80),
                eye: Color::Rgb(255, 220, 90),
                mouth: Color::Rgb(160, 220, 140),
            },
        }
    }
}

// --- rendering -------------------------------------------------------------

/// One canvas cell during compositing: `None` is transparent (scenery shows
/// through), `Some` is opaque.
type Cell = Option<(char, Color)>;

/// Left margin (and right) beside the centered face block.
const MARGIN: usize = (W - FACE) / 2;

pub fn render(mood: Mood, character: Character, frame: u64) -> Doodle {
    let t = frame as usize;
    let p = character.palette();
    let e = expression(mood, t);
    let (l, r) = character.sides();
    let scene = scene_for(mood);

    // Foreground: the creature + its mood particles. Transparent everywhere the
    // character isn't, so the backdrop can peek through.
    let mut fg: Vec<Vec<Cell>> = vec![vec![None; W]; 6];
    place_particles(&mut fg[0], &e.top, p.accent);
    place_particles(&mut fg[5], &e.bot, p.accent);
    let left = face_left(e.jit);
    place_face(&mut fg[1], left, &center_cells(character.top(), p.base));
    place_face(&mut fg[2], left, &eyes_cells(&e.le, &e.re, (l, r), &p));
    place_face(&mut fg[3], left, &mouth_cells(&e.mouth, (l, r), &p));
    place_face(&mut fg[4], left, &center_cells(character.base(), p.base));

    // Backdrop, composited underneath.
    let bg = background(&scene, t);
    let rows = (0..6)
        .map(|r| {
            let cells: Vec<(char, Color)> = (0..W)
                .map(|c| fg[r][c].or(bg[r][c]).unwrap_or((' ', Color::Reset)))
                .collect();
            coalesce_cells(cells)
        })
        .collect();

    Doodle {
        rows,
        name: character.name(),
        caption: caption(mood, t).to_string(),
        caption_color: p.accent,
        border_color: p.base,
    }
}

/// A mood's expression: eyes, mouth, the particle rows above and below, and a
/// horizontal jitter for the jittery moods. All animated by the clock `t`.
struct Expr {
    le: String,
    re: String,
    mouth: String,
    top: String,
    bot: String,
    jit: i32,
}

fn expression(mood: Mood, t: usize) -> Expr {
    // Helpers keep each arm to a readable line or two.
    let e = |le: &str, re: &str, mouth: &str, top: &str, bot: &str, jit: i32| Expr {
        le: le.into(),
        re: re.into(),
        mouth: mouth.into(),
        top: top.into(),
        bot: bot.into(),
        jit,
    };

    match mood {
        Mood::Sleepy => {
            let z = ["     z", "      z", "       Z", "        ."][(t / 4) % 4];
            e("-", "-", "~~", z, "", 0)
        }
        Mood::Bored => {
            let d = ["        .", "        . .", "        . . .", ""][(t / 6) % 4];
            e("o", "o", "__", d, "", 0)
        }
        Mood::Curious => {
            let (le, re) = if (t / 5) % 6 == 0 {
                ("-", "-")
            } else {
                ("o", "o")
            };
            let q = if (t / 6) % 2 == 0 {
                "        ?"
            } else {
                "          ?"
            };
            e(le, re, "..", q, "", 0)
        }
        Mood::Focused => {
            let spin = ['|', '/', '-', '\\'][(t / 2) % 4];
            let dots = ["     .", "     ..", "     ...", "     ...."][(t / 3) % 4];
            let top = format!("         {spin}");
            e("@", "@", "__", &top, dots, 0)
        }
        Mood::Toiling => {
            // Furrowed, gritted, sweating — and a slow chugging bar below.
            let grit = if (t / 3) % 2 == 0 { "><" } else { "--" };
            let sweat = ["  '        `", "     '   `", "        ' `", ""][(t / 3) % 4];
            let chug = ["  [=   ]", "  [==  ]", "  [=== ]", "  [====]"][(t / 4) % 4];
            e(">", "<", grit, sweat, chug, 0)
        }
        Mood::Caffeinated => {
            let jit = if t % 2 == 0 { 1 } else { -1 };
            let (top, bot) = if t % 2 == 0 {
                ("  ! *  ! * !", "  * !  * ! *")
            } else {
                (" * ! *  ! * ", " ! *  ! * ! ")
            };
            e("O", "O", "WW", top, bot, jit)
        }
        Mood::Happy => {
            let up = (t / 4) % 2 == 0;
            let (top, bot) = if up {
                ("   +        *", "      *")
            } else {
                (" *        +", "    +")
            };
            e("^", "^", "\\/", top, bot, 0)
        }
        Mood::Proud => {
            let top = if (t / 4) % 2 == 0 {
                "     *   *"
            } else {
                "      * *"
            };
            e("^", "^", "vv", top, "", 0)
        }
        Mood::Excited => {
            let ph = (t / 3) % 3;
            let top = ["  *   .   +", "  .   +   *", "  +   *   ."][ph];
            let bot = ["  +   *   .", "  *   .   +", "  .   +   *"][ph];
            e("*", "*", "oo", top, bot, 0)
        }
        Mood::Confused => {
            let ph = (t / 5) % 2 == 0;
            let (le, re) = if ph { ("o", "O") } else { ("O", "o") };
            let top = if ph { "    ? ! ?" } else { "    ! ? !" };
            let bot = if ph { "           '" } else { "            '" };
            e(le, re, "~~", top, bot, 0)
        }
    }
}

fn caption(mood: Mood, t: usize) -> &'static str {
    let opts: &[&str] = match mood {
        Mood::Sleepy => &["…dozing off…", "zzz…", "five more minutes…"],
        Mood::Bored => &["…meh…", "anything to run?", "tap tap tap…"],
        Mood::Curious => &[
            "what shall we run?",
            "ooh, what's this?",
            "show me something!",
        ],
        Mood::Focused => &["on it…", "crunching…", "hmm, let me see…"],
        Mood::Toiling => &[
            "still crunching…",
            "big one, huh…",
            "hang in there…",
            "almost, almost…",
        ],
        Mood::Caffeinated => &["gogogo!", "so many commands!!", "wheee~"],
        Mood::Happy => &["nice, that worked!", "clean run :)", "smooth."],
        Mood::Proud => &["nailed it.", "back on track!", "recovered like a pro."],
        Mood::Excited => &["ON A ROLL!!", "unstoppable!!", "yes yes YES"],
        Mood::Confused => &["hmm, that errored.", "wait, what?", "let's try again…"],
    };
    opts[(t / 24) % opts.len()]
}

// --- scenery ---------------------------------------------------------------

/// A backdrop behind the creature: sparse, gently-animated glyphs in the sky
/// band (top row), the ground band (bottom row), and the side margins. Kept
/// faint and thin so the character stays the star.
struct Scene {
    sky: &'static [char],
    ground: &'static [char],
    side: &'static [char],
    sky_color: Color,
    ground_color: Color,
    side_color: Color,
    /// Frames between glyph changes — lower is livelier.
    twinkle: usize,
}

/// Pick the backdrop that best fits the current mood.
fn scene_for(mood: Mood) -> Scene {
    match mood {
        // Night sky — quiet, drifting stars.
        Mood::Sleepy | Mood::Bored | Mood::Confused => Scene {
            sky: &['.', '*', '+', '\''],
            ground: &['_', '.'],
            side: &['.', '`'],
            sky_color: Color::Rgb(120, 140, 210),
            ground_color: Color::Rgb(70, 80, 120),
            side_color: Color::Rgb(90, 100, 150),
            twinkle: 14,
        },
        // Sunny meadow — warm, breezy.
        Mood::Happy | Mood::Curious | Mood::Proud => Scene {
            sky: &['*', '~', '\''],
            ground: &[',', '.', '\''],
            side: &['\'', '`'],
            sky_color: Color::Rgb(255, 210, 120),
            ground_color: Color::Rgb(150, 200, 120),
            side_color: Color::Rgb(200, 220, 150),
            twinkle: 12,
        },
        // Electric buzz — crackling energy.
        Mood::Caffeinated | Mood::Excited => Scene {
            sky: &['*', '+', '!', '.'],
            ground: &['.', '\''],
            side: &['/', '\\'],
            sky_color: Color::Rgb(255, 240, 120),
            ground_color: Color::Rgb(180, 140, 255),
            side_color: Color::Rgb(120, 230, 255),
            twinkle: 4,
        },
        // Deep focus / long toil — cool, minimal rails.
        Mood::Focused | Mood::Toiling => Scene {
            sky: &['.', '\''],
            ground: &['_', '-'],
            side: &['|', ':'],
            sky_color: Color::Rgb(110, 150, 170),
            ground_color: Color::Rgb(90, 110, 120),
            side_color: Color::Rgb(90, 120, 140),
            twinkle: 10,
        },
    }
}

/// A cheap deterministic hash — enough to scatter glyphs without an RNG.
fn hash(c: usize, salt: u64) -> u64 {
    let mut x = (c as u64)
        .wrapping_add(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ salt;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    x ^ (x >> 17)
}

/// A backdrop glyph for cell `c`, or `None` if this cell stays empty. Roughly
/// one cell in `density` carries a glyph; the glyph slowly twinkles with `t`.
fn mote(
    c: usize,
    glyphs: &[char],
    density: u64,
    salt: u64,
    t: usize,
    twinkle: usize,
) -> Option<char> {
    let h = hash(c, salt);
    if h % density != 0 {
        return None;
    }
    let phase = (h / density) as usize + t / twinkle.max(1);
    Some(glyphs[phase % glyphs.len()])
}

/// Build the 6×`W` backdrop layer for a scene at time `t`.
fn background(scene: &Scene, t: usize) -> Vec<Vec<Cell>> {
    let mut bg: Vec<Vec<Cell>> = vec![vec![None; W]; 6];

    for c in 0..W {
        if let Some(ch) = mote(c, scene.sky, 4, 0x5, t, scene.twinkle) {
            bg[0][c] = Some((ch, scene.sky_color));
        }
        if let Some(ch) = mote(c, scene.ground, 3, 0x9, t, scene.twinkle + 2) {
            bg[5][c] = Some((ch, scene.ground_color));
        }
    }

    // Side margins beside the face on rows 1..=4.
    for (row, cells) in bg.iter_mut().enumerate().take(5).skip(1) {
        for c in (0..MARGIN).chain(W - MARGIN..W) {
            if let Some(ch) = mote(c + row * 32, scene.side, 4, 0x11, t, scene.twinkle + 4) {
                cells[c] = Some((ch, scene.side_color));
            }
        }
    }
    bg
}

// --- canvas helpers --------------------------------------------------------

/// The centered face block's left edge, nudged by the mood's jitter.
fn face_left(jit: i32) -> usize {
    (MARGIN as i32 + jit).clamp(0, (W - FACE) as i32) as usize
}

/// Stamp a `FACE`-wide opaque block into a row at `left`.
fn place_face(row: &mut [Cell], left: usize, cells: &[(char, Color)]) {
    for (i, &cell) in cells.iter().enumerate() {
        if left + i < row.len() {
            row[left + i] = Some(cell);
        }
    }
}

/// Stamp a particle template across a full row: glyphs opaque, gaps transparent.
fn place_particles(row: &mut [Cell], tpl: &str, accent: Color) {
    for (c, ch) in tpl.chars().take(W).enumerate() {
        if ch != ' ' {
            row[c] = Some((ch, accent));
        }
    }
}

/// The eye line as `FACE` cells: walls in base, pupils in eye color.
fn eyes_cells(le: &str, re: &str, (l, r): (char, char), p: &Palette) -> Vec<(char, Color)> {
    let le = le.chars().next().unwrap_or('o');
    let re = re.chars().next().unwrap_or('o');
    vec![
        (l, p.base),
        (' ', p.base),
        (le, p.eye),
        (' ', p.base),
        (' ', p.base),
        (' ', p.base),
        (' ', p.base),
        (re, p.eye),
        (' ', p.base),
        (r, p.base),
    ]
}

/// The mouth line as `FACE` cells: walls in base, mouth in its color.
fn mouth_cells(m: &str, (l, r): (char, char), p: &Palette) -> Vec<(char, Color)> {
    let mut mc = m.chars();
    let m0 = mc.next().unwrap_or(' ');
    let m1 = mc.next().unwrap_or(' ');
    vec![
        (l, p.base),
        (' ', p.base),
        (' ', p.base),
        (' ', p.base),
        (m0, p.mouth),
        (m1, p.mouth),
        (' ', p.base),
        (' ', p.base),
        (' ', p.base),
        (r, p.base),
    ]
}

/// Center a string within the face block width as `FACE` colored cells.
fn center_cells(s: &str, color: Color) -> Vec<(char, Color)> {
    let n = s.chars().count().min(FACE);
    let total = FACE - n;
    let l = total / 2;
    let mut cells = vec![(' ', color); l];
    cells.extend(s.chars().take(FACE).map(|ch| (ch, color)));
    cells.extend(vec![(' ', color); total - l]);
    cells
}

/// Merge a row of `(char, color)` cells into color-grouped segments.
fn coalesce_cells(cells: Vec<(char, Color)>) -> Row {
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
