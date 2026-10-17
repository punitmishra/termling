//! Rendering: one full-width pane showing the ambient creature, the arcade
//! picker, or a game. (Your real shell lives in the neighbouring tmux/zellij
//! pane — termling never draws a shell of its own.)

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Flair, Mode};
use crate::buddy::{Quip, Tone};
use crate::doodle;
use crate::games::{GameKind, ItemColors};
use crate::panes::{Dashboard, Layout as PaneLayout, PaneKind};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    match app.mode() {
        Mode::Ambient => draw_doodle(f, app, area),
        Mode::Menu => draw_menu(f, app, area),
        Mode::Game => draw_game(f, app, area),
        Mode::Dashboard => draw_dashboard(f, app, area),
    }
}

/// The info dashboard, laid out like a terminal split: the buddy stays visible in
/// a compact strip on top, and the dashboard proper sits *under* it — a narrow
/// sidebar of pane names on the left, the content area tiled per the current
/// [`PaneLayout`] (`s` cycles: focused → grid → columns → rows) on the right.
fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let dash = app.dashboard();

    // Reserve a top band for the creature so it never fully disappears behind the
    // dashboard; the rest is the dashboard body. When the strip is hidden (`b`),
    // the panes get the whole area instead.
    let body = if dash.strip_hidden() {
        area
    } else {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)])
            .split(area);
        draw_creature_strip(f, app, split[0]);
        split[1]
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(0)])
        .split(body);
    draw_sidebar(f, dash, cols[0], &theme);

    let content = cols[1];
    let layout = dash.layout();
    let cells = content_cells(layout, content);
    // Window the panes onto the page that contains the selection, so the whole
    // set (including the online panes further down) is reachable in every layout.
    let n = cells.len().max(1);
    let start = window_start(dash.sel(), n);
    for ((i, (kind, pane)), cell) in dash.all().enumerate().skip(start).take(n).zip(cells) {
        draw_pane(f, kind, pane, cell, &theme, i == dash.sel());
    }
}

/// The index the current page starts at: the page of `cells` panes that contains
/// `sel`. Anchoring on the selection keeps the highlighted pane (including online
/// panes further down the list) on screen in every multi-pane layout.
fn window_start(sel: usize, cells: usize) -> usize {
    let n = cells.max(1);
    (sel / n) * n
}

/// The compact creature band shown atop the dashboard: the current character in
/// its mood, plus a one-line level/XP/streak HUD — a smaller cousin of the full
/// ambient view, so the buddy stays in view while you browse panes.
fn draw_creature_strip(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let mood = app.mood();
    let character = app.current_character();
    let d = doodle::render(mood, character, app.frame);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} · {} ", d.name, mood.label()))
        .border_style(Style::default().fg(d.border_color));

    let mut lines: Vec<Line> = Vec::new();
    for row in &d.rows {
        let spans: Vec<Span> = row
            .iter()
            .map(|s| Span::styled(s.text.clone(), Style::default().fg(s.color)))
            .collect();
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!("lv {}  ", app.level()),
            Style::default().fg(theme.accent),
        ),
        xp_bar(app.level_progress(), 8, theme.bar),
        Span::styled(
            format!("  {}xp · streak {}", app.xp(), app.streak()),
            Style::default().fg(theme.dim),
        ),
    ]));

    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

/// Split the content area into the tiles for a layout: one cell (focused), a 2×2
/// grid, N side-by-side columns, or N stacked rows.
fn content_cells(layout: PaneLayout, content: Rect) -> Vec<Rect> {
    let even = |n: usize| vec![Constraint::Ratio(1, n as u32); n];
    match layout {
        PaneLayout::Focused => vec![content],
        PaneLayout::Grid => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content)
            .iter()
            .flat_map(|r| {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(*r)
                    .to_vec()
            })
            .collect(),
        PaneLayout::Columns => Layout::default()
            .direction(Direction::Horizontal)
            .constraints(even(layout.cells()))
            .split(content)
            .to_vec(),
        PaneLayout::Rows => Layout::default()
            .direction(Direction::Vertical)
            .constraints(even(layout.cells()))
            .split(content)
            .to_vec(),
    }
}

/// The dashboard's left sidebar: one line per pane, the selected one highlighted,
/// online panes flagged so it's obvious which need `TERMLING_ONLINE=1`.
fn draw_sidebar(f: &mut Frame, dash: &Dashboard, area: Rect, theme: &crate::theme::Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" panes ")
        .border_style(Style::default().fg(theme.accent));

    let mut lines: Vec<Line> = Vec::new();
    for (i, kind) in dash.kinds().iter().enumerate() {
        let selected = i == dash.sel();
        let marker = if selected { "▸ " } else { "  " };
        let suffix = if kind.online() { " ·" } else { "" };
        let style = if selected {
            Style::default().fg(theme.bar).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.accent)
        };
        lines.push(Line::styled(
            format!("{marker}{}{suffix}", kind.name()),
            style,
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("s {}", dash.layout().label()),
        Style::default().fg(theme.dim),
    ));
    lines.push(Line::styled(
        if dash.strip_hidden() {
            "b show buddy"
        } else {
            "b hide buddy"
        },
        Style::default().fg(theme.dim),
    ));
    lines.push(Line::styled("r refresh", Style::default().fg(theme.dim)));
    lines.push(Line::styled("esc back", Style::default().fg(theme.dim)));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render one pane into `area`: a bordered, titled box filled with the pane's own
/// colored rows (shared `Row`/`Seg` vocabulary), clipped to the tile.
fn draw_pane(
    f: &mut Frame,
    _kind: PaneKind,
    pane: &dyn crate::panes::Pane,
    area: Rect,
    theme: &crate::theme::Theme,
    selected: bool,
) {
    // The selected tile gets the bar color + a ▸ marker so it stands out in the
    // grid; the rest keep the quiet accent border.
    let (border, title) = if selected {
        (
            Style::default().fg(theme.bar).add_modifier(Modifier::BOLD),
            format!(" ▸ {} ", pane.title()),
        )
    } else {
        (
            Style::default().fg(theme.accent),
            format!(" {} ", pane.title()),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border);
    let inner = block.inner(area);

    let rows = pane.render(inner.width, inner.height, theme);
    let lines: Vec<Line> = rows
        .iter()
        .take(inner.height as usize)
        .map(|row| {
            let spans: Vec<Span> = row
                .iter()
                .map(|s| Span::styled(s.text.clone(), Style::default().fg(s.color)))
                .collect();
            Line::from(spans)
        })
        .collect();

    f.render_widget(Paragraph::new(lines).block(block), area);
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

    // The buddy's speech bubble floats just above the creature. When it has
    // nothing to say, a single blank line keeps the layout from jumping.
    let mut lines: Vec<Line> = match app.speech() {
        Some(quip) => speech_bubble(quip, &theme),
        None => vec![Line::raw("")],
    };
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
    // What the buddy brain has picked up — a quiet sign it's learning your tools.
    let learned = app.buddy().known_tools();
    if learned > 0 {
        lines.push(Line::styled(
            format!(
                "buddy knows {} tool{}",
                learned,
                if learned == 1 { "" } else { "s" }
            ),
            Style::default().fg(theme.dim),
        ));
    }

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

/// Build the buddy's speech bubble: a rounded box holding one short line, tinted
/// by the quip's tone, with a little tail pointing down at the creature. Returned
/// as ready-to-render centered lines.
fn speech_bubble(quip: &Quip, theme: &crate::theme::Theme) -> Vec<Line<'static>> {
    let color = tone_color(quip.tone, theme);
    let style = Style::default().fg(color);
    // Cap the width so a long quip never blows out the pane. Measured in display
    // *cells*, not code points, so a wide/emoji-heavy quip can't still overflow.
    let text = clip_to_cells(&quip.text, 30);
    // Size the border by display width too: the buddy's quips carry emoji (🔮 👀 ⚡)
    // and tool names may hold wide/CJK glyphs, all of which occupy two terminal
    // cells — counting chars would leave the right edge short of the content box.
    let inner = display_width(&text);
    let bar: String = "─".repeat(inner + 2);

    vec![
        Line::styled(format!("╭{bar}╮"), style),
        Line::styled(format!("│ {text} │"), style),
        Line::styled(format!("╰{bar}╯"), style),
        Line::styled("▾", style),
    ]
}

/// A pragmatic terminal display width in cells. We don't pull in a full
/// Unicode-width table as a dependency (against the crate's zero-dep ethos); this
/// hand-rolled version covers every glyph that can reach the speech bubble — the
/// buddy's own emoji plus arbitrary (possibly non-English) tool names — and
/// matches what ratatui's layout (unicode-width) does for them, so the boxed
/// border and the centered content line come out the same width.
///
/// Width 2: East-Asian wide / fullwidth blocks (CJK, kana, Hangul, …) and the
/// emoji / emoji-presentation symbol blocks. Width 0: zero-width joiners,
/// variation selectors, and combining marks. Everything else is 1. The only
/// snag is that the symbol blocks mix emoji-presentation glyphs (two cells, e.g.
/// ✅ ❌ ⭐ ⚡) with a few text-presentation ones; the buddy renders exactly one of
/// the latter — `✓` — so it's listed as a one-cell exception.
fn display_width(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

fn char_cells(c: char) -> usize {
    let cp = c as u32;
    // Zero-width: joiners, variation selectors, combining marks.
    if cp == 0x200D || (0xFE00..=0xFE0F).contains(&cp) || (0x0300..=0x036F).contains(&cp) {
        return 0;
    }
    // Text-presentation symbols the buddy uses that sit inside the "wide" symbol
    // blocks below but render as a single cell. `✓` (U+2713) is the only one.
    if cp == 0x2713 {
        return 1;
    }
    // Blocks that render two cells wide.
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115F),   // Hangul Jamo
        (0x2600, 0x27BF),   // Misc symbols + Dingbats (⚡ ✅ ❌ …)
        (0x2B00, 0x2BFF),   // Misc symbols & arrows (⭐ …)
        (0x2E80, 0x303E),   // CJK radicals … Kangxi
        (0x3041, 0x33FF),   // kana … CJK compatibility
        (0x3400, 0x4DBF),   // CJK Unified Ext A
        (0x4E00, 0x9FFF),   // CJK Unified
        (0xA000, 0xA4CF),   // Yi
        (0xAC00, 0xD7A3),   // Hangul syllables
        (0xF900, 0xFAFF),   // CJK compatibility ideographs
        (0xFE30, 0xFE4F),   // CJK compatibility forms
        (0xFF00, 0xFF60),   // fullwidth forms
        (0xFFE0, 0xFFE6),   // fullwidth signs
        (0x1F000, 0x1FAFF), // emoji (supplementary plane)
        (0x20000, 0x3FFFD), // CJK Unified Ext B and beyond
    ];
    if WIDE.iter().any(|(lo, hi)| cp >= *lo && cp <= *hi) {
        return 2;
    }
    1
}

/// Clip `s` to at most `max` display *cells* (not code points), appending `…` when
/// it overflows. Mirrors [`truncate`] but respects wide glyphs so the result can't
/// exceed the pane.
fn clip_to_cells(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    // Leave a cell for the ellipsis.
    let budget = max.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_cells(c);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Map a quip's tone to a palette entry.
fn tone_color(tone: Tone, theme: &crate::theme::Theme) -> Color {
    match tone {
        Tone::Cheer => theme.good,
        Tone::Worry => theme.bad,
        Tone::Insight => theme.gem,
        Tone::Neutral => theme.accent,
    }
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
    // Count line + options line + controls line.
    const FOOTER: usize = 3;
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
    // The currently-selected options, so the player sees what they'll launch.
    let opts = app.game_options();
    lines.push(Line::styled(
        format!(
            "difficulty {}  ·  speed {}",
            opts.difficulty.label(),
            opts.speed.label()
        ),
        Style::default().fg(theme.bar),
    ));
    lines.push(Line::styled(
        "↑↓ pick  ·  d difficulty  ·  s speed  ·  enter play  ·  esc back",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_matches_terminal_cells() {
        // ASCII stays one cell each.
        assert_eq!(display_width("cargo"), 5);
        // `✓` is a one-cell (text-presentation) check mark despite sitting in the
        // symbol block; emoji-presentation symbols in the same block are two cells.
        assert_eq!(display_width("✓ nice"), 6);
        assert_eq!(display_width("⚡"), 2);
        assert_eq!(display_width("✅"), 2);
        assert_eq!(display_width("⭐"), 2);
        assert_eq!(display_width("❌"), 2);
        // Supplementary-plane emoji the buddy uses.
        assert_eq!(display_width("🔮"), 2);
        assert_eq!(display_width("👀"), 2);
        // East-Asian wide / CJK tool names count as two cells each (review finding
        // #1): `文档` is four cells, not two.
        assert_eq!(display_width("文档"), 4);
        assert_eq!(display_width("かな"), 4);
        // A realistic quip: adding a 2-cell emoji adds exactly 2.
        assert_eq!(display_width("`docker` 👀"), display_width("`docker` ") + 2);
    }

    #[test]
    fn window_anchors_on_selection() {
        // 4-cell grid: the first page covers 0..4.
        assert_eq!(window_start(0, 4), 0);
        assert_eq!(window_start(3, 4), 0);
        // Selecting a later pane (e.g. an online pane at index 5) pages forward
        // and still includes the selection in the window.
        let start = window_start(5, 4);
        assert_eq!(start, 4);
        assert!((start..start + 4).contains(&5));
        // A 3-cell layout (columns/rows) pages in threes.
        assert_eq!(window_start(7, 3), 6);
        assert!((window_start(7, 3)..window_start(7, 3) + 3).contains(&7));
    }

    #[test]
    fn clip_to_cells_respects_wide_glyphs() {
        // Short input is returned unchanged.
        assert_eq!(clip_to_cells("hi", 30), "hi");
        // A wide-glyph string is capped by *cells*: the result (incl. the `…`)
        // never exceeds the budget, though the char count alone would overflow.
        let wide = "文".repeat(40); // 80 cells
        let clipped = clip_to_cells(&wide, 30);
        assert!(display_width(&clipped) <= 30);
        assert!(clipped.ends_with('…'));
    }
}
