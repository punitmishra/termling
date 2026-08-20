//! Rendering: one full-width pane showing the ambient creature, the arcade
//! picker, or a game. (Your real shell lives in the neighbouring tmux/zellij
//! pane — termling never draws a shell of its own.)

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Flair, Mode};
use crate::doodle;
use crate::games::{GameKind, ItemColors};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    match app.mode() {
        Mode::Ambient => draw_doodle(f, app, area),
        Mode::Menu => draw_menu(f, app, area),
        Mode::Game => draw_game(f, app, area),
    }
}

fn draw_doodle(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let mood = app.mood();
    // Whoever is on stage this frame from the rotating cast.
    let character = app.current_character();
    let d = doodle::render(mood, character, app.frame);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} · {} ", d.name, mood.label()))
        .border_style(Style::default().fg(d.border_color));

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for row in &d.rows {
        let spans: Vec<Span> = row
            .iter()
            .map(|s| Span::styled(s.text.clone(), Style::default().fg(s.color)))
            .collect();
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        d.caption.clone(),
        Style::default()
            .fg(d.caption_color)
            .add_modifier(Modifier::ITALIC),
    ));

    // The gamified-shell HUD: level, an XP bar, current + best streak.
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(
            format!("lv {}  ", app.level()),
            Style::default().fg(theme.accent),
        ),
        xp_bar(app.level_progress(), 8, theme.bar),
        Span::styled(
            format!("  {}xp", app.xp()),
            Style::default().fg(theme.accent),
        ),
    ]));
    lines.push(Line::styled(
        format!("streak {}  ·  best {}", app.streak(), app.best_streak()),
        Style::default().fg(theme.dim),
    ));

    // A brief celebratory flourish (level-up, streak milestone, long job done).
    if let Some(flair) = app.flair() {
        let (text, color) = flair_banner(flair, app, &theme);
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            confetti(app.frame),
            Style::default().fg(theme.gem),
        ));
        lines.push(Line::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }

    // The recent-command feed — this is what makes the pane visibly track your
    // real shell. Oldest first, so the newest lands nearest the controls line.
    let recent = app.recent();
    if !recent.is_empty() {
        lines.push(Line::raw(""));
        for (cmd, code) in recent {
            let (glyph, gcolor) = result_glyph(*code, &theme);
            lines.push(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(gcolor)),
                Span::styled(truncate(cmd, 22), Style::default().fg(theme.dim)),
            ]));
        }
    }

    // Current project directory.
    if let Some(dir) = app.cwd() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("in {}", truncate(basename(dir), 22)),
            Style::default().fg(theme.accent),
        ));
    }

    // The multi-shell roster — one line per shell, when more than one is around.
    let shells = app.shells();
    if shells.len() > 1 {
        lines.push(Line::styled(
            format!("shells ({})", shells.len()),
            Style::default().fg(theme.dim),
        ));
        for s in shells {
            let (glyph, gcolor) = if s.running {
                ("▶", theme.accent)
            } else {
                let (g, c) = result_glyph(s.last_exit, &theme);
                (g, c)
            };
            let where_ = s.cwd.as_deref().map(basename).unwrap_or("·");
            lines.push(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(gcolor)),
                Span::styled(truncate(where_, 20), Style::default().fg(theme.dim)),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("g play · t {} · q quit", theme.name),
        Style::default().fg(theme.dim),
    ));

    // No wrap: trimming would strip the leading spaces that place each particle.
    // Every row is a fixed-width block, so center alignment keeps it steady.
    let para = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(para, area);
}

/// The banner text + color for an active celebratory flourish.
fn flair_banner(flair: Flair, app: &App, theme: &crate::theme::Theme) -> (String, Color) {
    match flair {
        Flair::LevelUp => (format!("✦ LEVEL {} ✦", app.level()), theme.gem),
        Flair::Milestone => (format!("✦ STREAK x{} ✦", app.streak()), theme.good),
        Flair::LongDone => ("✓ done — nice work!".to_string(), theme.good),
    }
}

/// A little animated confetti strip, drifting with the animation clock.
fn confetti(frame: u64) -> String {
    const GLYPHS: [char; 6] = ['*', '+', '✦', '·', '˚', '.'];
    let off = (frame / 2) as usize;
    (0..11)
        .map(|c| GLYPHS[(c + off) % GLYPHS.len()])
        .collect::<String>()
        .chars()
        .flat_map(|c| [c, ' '])
        .collect()
}

/// The last path component of a directory, for compact display.
fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

/// The glyph + color for a command outcome: `…` still running, `✓` ok, `✗` failed.
fn result_glyph(code: Option<i32>, theme: &crate::theme::Theme) -> (&'static str, Color) {
    match code {
        None => ("…", theme.dim),
        Some(0) => ("✓", theme.good),
        Some(_) => ("✗", theme.bad),
    }
}

/// A little `█████░░░` XP meter, filled in the theme's bar color.
fn xp_bar((cur, need): (u32, u32), width: usize, color: Color) -> Span<'static> {
    let filled = ((cur as usize * width) / need.max(1) as usize).min(width);
    let mut s = String::new();
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..width {
        s.push('░');
    }
    Span::styled(s, Style::default().fg(color))
}

/// The arcade picker: a scrolling list of games with the highlighted one marked.
/// The list windows to whatever height the pane offers, so the catalogue can
/// grow without ever clipping the selection.
fn draw_menu(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" arcade ")
        .border_style(Style::default().fg(theme.accent));
    let inner_h = block.inner(area).height as usize;

    let games = GameKind::ALL;
    let sel = app.menu_sel();

    // Fixed chrome: a "choose a game" header line and a controls footer line,
    // each with a blank spacer. Everything else is the windowed game list, at
    // two rows per entry (name + tagline).
    const HEADER: usize = 2;
    const FOOTER: usize = 2;
    const PER_ENTRY: usize = 2;
    let list_rows = inner_h.saturating_sub(HEADER + FOOTER);
    let fit = (list_rows / PER_ENTRY).max(1);

    // Scroll so the selected entry is always inside the window.
    let start = if sel >= fit { sel - fit + 1 } else { 0 };
    let end = (start + fit).min(games.len());

    let mut lines: Vec<Line> = vec![
        Line::styled("choose a game", Style::default().fg(theme.accent)),
        Line::styled(
            if start > 0 { "▲ more above" } else { "" }.to_string(),
            Style::default().fg(theme.dim),
        ),
    ];

    for (i, kind) in games.iter().enumerate().take(end).skip(start) {
        let is_sel = i == sel;
        let marker = if is_sel { "▸ " } else { "  " };
        let style = if is_sel {
            Style::default().fg(theme.bar).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent)
        };
        lines.push(Line::styled(format!("{marker}{}", kind.name()), style));
        lines.push(Line::styled(
            format!("   {}", kind.tagline()),
            Style::default().fg(theme.dim),
        ));
    }

    lines.push(Line::styled(
        if end < games.len() {
            format!("▼ more below ({}/{})", sel + 1, games.len())
        } else {
            format!("{}/{}", sel + 1, games.len())
        },
        Style::default().fg(theme.dim),
    ));
    lines.push(Line::styled(
        "↑↓ pick  ·  enter play  ·  esc back",
        Style::default().fg(theme.dim),
    ));

    let para = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(para, area);
}

fn draw_game(f: &mut Frame, app: &App, area: Rect) {
    let Some(g) = app.game() else { return };
    let theme = app.theme();
    let title = GameKind::ALL[app.menu_sel()].name();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} · {} ", title, g.character_name()))
        .border_style(Style::default().fg(theme.accent));

    let mut lines: Vec<Line> = Vec::new();

    // Top HUD: score, level, and (if the game has them) lives as hearts.
    let mut hud_spans = vec![Span::styled(
        format!("score {:>4}   lv {}   ", g.score(), g.level()),
        Style::default().fg(theme.accent),
    )];
    if let Some(lives) = g.lives() {
        hud_spans.push(Span::styled(
            lives_bar(lives),
            Style::default().fg(theme.bad),
        ));
    }
    lines.push(Line::from(hud_spans));
    lines.push(Line::styled(g.hud(), Style::default().fg(theme.dim)));
    lines.push(Line::raw(""));

    // The play-field, tinted with the active theme's item colors.
    let colors = ItemColors {
        good: theme.good,
        gem: theme.gem,
        bad: theme.bad,
    };
    for row in g.render(colors) {
        let spans: Vec<Span> = row
            .iter()
            .map(|s| Span::styled(s.text.clone(), Style::default().fg(s.color)))
            .collect();
        lines.push(Line::from(spans));
    }

    // Footer / controls.
    lines.push(Line::raw(""));
    if g.is_over() {
        lines.push(Line::styled(
            "GAME OVER",
            Style::default().fg(theme.bad).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            "r retry  ·  esc back",
            Style::default().fg(theme.dim),
        ));
    } else {
        lines.push(Line::styled(g.controls(), Style::default().fg(theme.dim)));
    }

    let para = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(para, area);
}

/// Remaining lives as hearts.
fn lives_bar(lives: u32) -> String {
    "♥ ".repeat(lives as usize).trim_end().to_string()
}

/// Clip `s` to at most `max` chars, appending an ellipsis when it overflows.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let keep = max.saturating_sub(1);
        format!("{}…", s.chars().take(keep).collect::<String>())
    }
}
