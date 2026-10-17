//! The dashboard pane framework.
//!
//! A [`Pane`] is one tile in termling's sidebar-driven dashboard. The design
//! mirrors [`crate::games`]: a trait plus a [`PaneKind`] registry, so adding a
//! pane is a new file and one line in [`PaneKind::ALL`]/[`PaneKind::make`]. Panes
//! render into the shared [`Row`]/[`Seg`](crate::doodle) vocabulary the rest of
//! the UI already speaks, so the dashboard composes them with no special casing.
//!
//! Local panes (git, system, buddy insights) work in the default zero-network
//! build. *Online* panes (weather, stocks, news, dev-news, AI) are compiled only
//! `--features online` and fetch through [`crate::net`] off the render thread:
//! each tick the dashboard asks a pane for an optional [`FetchSpec`] via
//! [`Pane::poll_fetch`], the worker performs it, and the result is folded back in
//! through [`Pane::on_data`]. A pane never blocks and never calls the network
//! itself.

mod bg;
mod buddy_insights;
mod clock;
mod git;
mod system;

use crate::doodle::{Row, Seg};
use crate::net::{FetchResult, FetchSpec};
use crate::theme::Theme;
use ratatui::style::Color;

#[cfg(feature = "online")]
mod ai;
#[cfg(feature = "online")]
mod devnews;
#[cfg(feature = "online")]
mod news;
#[cfg(feature = "online")]
mod stocks;
#[cfg(feature = "online")]
mod weather;

/// Every pane in the dashboard. Add a variant here (plus a file implementing
/// [`Pane`]) and wire it into [`ALL`](PaneKind::ALL), [`name`](PaneKind::name),
/// [`online`](PaneKind::online), and [`make`](PaneKind::make).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// A built-in welcome / help tile (always present).
    Welcome,
    /// A local git-repository summary for the current working dir.
    Git,
    /// Local date, time, and a current-month calendar.
    Clock,
    /// Local machine vitals: load average, uptime, memory.
    System,
    /// A read-only window onto the buddy brain's learned habits.
    BuddyInsights,
    /// Current conditions from Open-Meteo.
    #[cfg(feature = "online")]
    Weather,
    /// Quotes from Stooq.
    #[cfg(feature = "online")]
    Stocks,
    /// Top stories from Hacker News.
    #[cfg(feature = "online")]
    News,
    /// Hottest stories from Lobsters.
    #[cfg(feature = "online")]
    DevNews,
    /// A Claude-backed tip tile (only with `--features online`).
    #[cfg(feature = "online")]
    Ai,
}

impl PaneKind {
    /// The panes available in this build, in sidebar order. Online panes only
    /// appear when compiled `--features online`.
    pub fn all() -> Vec<PaneKind> {
        #[allow(unused_mut)]
        let mut kinds = vec![
            PaneKind::Welcome,
            PaneKind::Git,
            PaneKind::Clock,
            PaneKind::System,
            PaneKind::BuddyInsights,
        ];
        #[cfg(feature = "online")]
        {
            kinds.push(PaneKind::Weather);
            kinds.push(PaneKind::Stocks);
            kinds.push(PaneKind::News);
            kinds.push(PaneKind::DevNews);
            kinds.push(PaneKind::Ai);
        }
        kinds
    }

    /// Resolve a pane by its [`name`](PaneKind::name) — for `TERMLING_PANES`.
    /// Returns `None` for unknown names and for online panes in a build without
    /// the `online` feature, so configuration silently skips what isn't available.
    pub fn from_name(name: &str) -> Option<PaneKind> {
        match name {
            "welcome" => Some(PaneKind::Welcome),
            "git" => Some(PaneKind::Git),
            "clock" => Some(PaneKind::Clock),
            "system" => Some(PaneKind::System),
            "buddy" => Some(PaneKind::BuddyInsights),
            #[cfg(feature = "online")]
            "weather" => Some(PaneKind::Weather),
            #[cfg(feature = "online")]
            "stocks" => Some(PaneKind::Stocks),
            #[cfg(feature = "online")]
            "news" => Some(PaneKind::News),
            #[cfg(feature = "online")]
            "devnews" => Some(PaneKind::DevNews),
            #[cfg(feature = "online")]
            "ai" | "ai tip" => Some(PaneKind::Ai),
            _ => None,
        }
    }

    /// Short sidebar label.
    pub fn name(self) -> &'static str {
        match self {
            PaneKind::Welcome => "welcome",
            PaneKind::Git => "git",
            PaneKind::Clock => "clock",
            PaneKind::System => "system",
            PaneKind::BuddyInsights => "buddy",
            #[cfg(feature = "online")]
            PaneKind::Weather => "weather",
            #[cfg(feature = "online")]
            PaneKind::Stocks => "stocks",
            #[cfg(feature = "online")]
            PaneKind::News => "news",
            #[cfg(feature = "online")]
            PaneKind::DevNews => "devnews",
            #[cfg(feature = "online")]
            PaneKind::Ai => "ai tip",
        }
    }

    /// Whether this pane needs the network (and thus online mode to do anything).
    pub fn online(self) -> bool {
        match self {
            PaneKind::Welcome => false,
            PaneKind::Git => false,
            PaneKind::Clock => false,
            PaneKind::System => false,
            PaneKind::BuddyInsights => false,
            #[cfg(feature = "online")]
            PaneKind::Weather
            | PaneKind::Stocks
            | PaneKind::News
            | PaneKind::DevNews
            | PaneKind::Ai => true,
        }
    }

    /// Construct the live pane.
    pub fn make(self) -> Box<dyn Pane> {
        match self {
            PaneKind::Welcome => Box::new(Welcome),
            PaneKind::Git => Box::new(git::GitPane::new()),
            PaneKind::Clock => Box::new(clock::ClockPane::new()),
            PaneKind::System => Box::new(system::SystemPane::new()),
            PaneKind::BuddyInsights => Box::new(buddy_insights::BuddyPane::new()),
            #[cfg(feature = "online")]
            PaneKind::Weather => Box::new(weather::Weather::new()),
            #[cfg(feature = "online")]
            PaneKind::Stocks => Box::new(stocks::Stocks::new()),
            #[cfg(feature = "online")]
            PaneKind::News => Box::new(news::News::new()),
            #[cfg(feature = "online")]
            PaneKind::DevNews => Box::new(devnews::DevNews::new()),
            #[cfg(feature = "online")]
            PaneKind::Ai => Box::new(ai::Ai::new()),
        }
    }
}

/// One dashboard tile. Local panes implement only `title`/`render`; networked
/// panes also implement `poll_fetch`/`on_data` to drive [`crate::net`].
pub trait Pane {
    /// Heading shown at the top of the tile.
    fn title(&self) -> String;

    /// Paint the tile's body into at most `width`×`height` cells, in the active
    /// theme. Returns shared colored [`Row`]s; the UI clips to the tile.
    fn render(&self, width: u16, height: u16, theme: &Theme) -> Vec<Row>;

    /// Advance any animation / mark data stale. Called once per dashboard tick.
    fn tick(&mut self, _frame: u64) {}

    /// Force a re-fetch / re-collect on the next tick. Online panes reset their
    /// staleness (and the AI pane re-arms its one-shot); local panes drop their
    /// refresh cadence so they recollect. Default: no-op.
    fn refresh(&mut self) {}

    /// A network request this pane wants made *now*, if its data is due. Local
    /// panes return `None`. The dashboard only forwards these in online mode.
    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        None
    }

    /// Fold a completed fetch (routed to this pane by kind) back into state.
    fn on_data(&mut self, _result: &FetchResult) {}
}

/// The panes to show, in order. `TERMLING_PANES` (a comma-separated list of pane
/// names, e.g. `git,weather,ai`) picks and orders them; unknown or unavailable
/// names are skipped. Unset or empty → the full default set ([`PaneKind::all`]).
fn configured_kinds() -> Vec<PaneKind> {
    if let Ok(spec) = std::env::var("TERMLING_PANES") {
        let chosen: Vec<PaneKind> = spec
            .split(',')
            .filter_map(|n| PaneKind::from_name(n.trim()))
            .collect();
        if !chosen.is_empty() {
            return chosen;
        }
    }
    PaneKind::all()
}

/// Build a one-line [`Row`] of a single color — the common case for text panes.
pub fn line(text: impl Into<String>, color: Color) -> Row {
    vec![Seg {
        text: text.into(),
        color,
    }]
}

/// Collapse a possibly-multi-line, possibly-long error into one short line fit
/// for a narrow tile: the first line, capped at 56 chars. Shared by the online
/// panes' `on_data` error handling.
#[cfg(feature = "online")]
pub(crate) fn short(e: &str) -> String {
    let first = e.trim().lines().next().unwrap_or("").trim();
    first.chars().take(56).collect()
}

/// Clip a string to `max` columns (by char), adding an ellipsis when cut. Shared
/// by the headline panes.
#[cfg(feature = "online")]
pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let take = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(take).collect();
    out.push('…');
    out
}

/// How the dashboard content area is tiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One pane fills the content area.
    Focused,
    /// A 2×2 grid.
    Grid,
    /// Panes side by side (vertical dividers).
    Columns,
    /// Panes stacked (horizontal dividers).
    Rows,
}

impl Layout {
    /// Cycle order for the `s` key.
    pub fn next(self) -> Layout {
        match self {
            Layout::Focused => Layout::Grid,
            Layout::Grid => Layout::Columns,
            Layout::Columns => Layout::Rows,
            Layout::Rows => Layout::Focused,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Layout::Focused => "focused",
            Layout::Grid => "grid",
            Layout::Columns => "columns",
            Layout::Rows => "rows",
        }
    }

    /// How many panes this layout shows at once (a window around the selection).
    pub fn cells(self) -> usize {
        match self {
            Layout::Focused => 1,
            Layout::Grid => 4,
            Layout::Columns | Layout::Rows => 3,
        }
    }
}

/// The dashboard: the set of live panes, the sidebar selection, and how the
/// content area is tiled. Lives on [`App`](crate::app::App) like the arcade game.
pub struct Dashboard {
    kinds: Vec<PaneKind>,
    panes: Vec<Box<dyn Pane>>,
    sel: usize,
    layout: Layout,
}

impl Dashboard {
    pub fn new() -> Dashboard {
        let kinds = configured_kinds();
        let panes = kinds.iter().map(|k| k.make()).collect();
        Dashboard {
            kinds,
            panes,
            sel: 0,
            layout: Layout::Focused,
        }
    }

    pub fn kinds(&self) -> &[PaneKind] {
        &self.kinds
    }

    pub fn sel(&self) -> usize {
        self.sel
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Step to the next content layout (the `s` key).
    pub fn cycle_layout(&mut self) {
        self.layout = self.layout.next();
    }

    pub fn next(&mut self) {
        if !self.panes.is_empty() {
            self.sel = (self.sel + 1) % self.panes.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.panes.is_empty() {
            let n = self.panes.len();
            self.sel = (self.sel + n - 1) % n;
        }
    }

    /// All panes paired with their kind, for grid rendering.
    pub fn all(&self) -> impl Iterator<Item = (PaneKind, &dyn Pane)> {
        self.kinds
            .iter()
            .copied()
            .zip(self.panes.iter().map(|b| b.as_ref()))
    }

    /// Advance every pane's animation clock.
    pub fn tick(&mut self, frame: u64) {
        for p in &mut self.panes {
            p.tick(frame);
        }
    }

    /// Ask every pane to re-fetch / re-collect on its next tick (the `r` key).
    pub fn refresh_all(&mut self) {
        for p in &mut self.panes {
            p.refresh();
        }
    }

    /// Collect any fetches the panes want performed this tick (online mode only).
    pub fn poll_fetches(&mut self) -> Vec<FetchSpec> {
        self.panes
            .iter_mut()
            .filter_map(|p| p.poll_fetch())
            .collect()
    }

    /// Route a completed fetch to the pane that asked for it.
    pub fn on_data(&mut self, result: &FetchResult) {
        for (kind, pane) in self.kinds.iter().zip(self.panes.iter_mut()) {
            if *kind == result.pane {
                pane.on_data(result);
            }
        }
    }
}

impl Default for Dashboard {
    fn default() -> Self {
        Dashboard::new()
    }
}

/// The built-in welcome tile: a short orientation + what the dashboard can do.
struct Welcome;

impl Pane for Welcome {
    fn title(&self) -> String {
        "welcome".into()
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        vec![
            line("termling dashboard", theme.accent),
            line("", theme.dim),
            line("↑/↓  pick a pane", theme.dim),
            line("s    cycle layout", theme.dim),
            line("r    refresh", theme.dim),
            line("esc  back to the creature", theme.dim),
            line("", theme.dim),
            line("more panes arrive as", theme.dim),
            line("features land.", theme.dim),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    #[test]
    fn dashboard_starts_with_the_welcome_pane_and_renders() {
        let dash = Dashboard::new();
        assert_eq!(dash.sel(), 0);
        assert_eq!(dash.layout(), Layout::Focused);
        let (_, pane) = dash.all().next().expect("a pane");
        assert_eq!(pane.title(), "welcome");
        let rows = pane.render(40, 12, &THEMES[0]);
        assert!(!rows.is_empty());
    }

    #[test]
    fn sidebar_navigation_wraps_and_layout_cycles() {
        let mut dash = Dashboard::new();
        let n = dash.kinds().len();
        dash.prev(); // wrap backwards from 0
        assert_eq!(dash.sel(), n - 1);
        dash.next(); // back to 0
        assert_eq!(dash.sel(), 0);
        // Layout cycles focused → grid → columns → rows → focused.
        assert_eq!(dash.layout(), Layout::Focused);
        dash.cycle_layout();
        assert_eq!(dash.layout(), Layout::Grid);
        dash.cycle_layout();
        assert_eq!(dash.layout(), Layout::Columns);
        dash.cycle_layout();
        assert_eq!(dash.layout(), Layout::Rows);
        dash.cycle_layout();
        assert_eq!(dash.layout(), Layout::Focused);
    }

    #[test]
    fn configured_panes_select_and_order() {
        // from_name resolves local panes and rejects unknowns.
        assert_eq!(PaneKind::from_name("git"), Some(PaneKind::Git));
        assert_eq!(PaneKind::from_name("nope"), None);
    }

    #[test]
    fn local_panes_request_no_network() {
        // A *local* pane must never ask the framework to fetch anything — build
        // each one directly so the check is independent of the environment (an
        // online pane may legitimately want the network when a key is present).
        for kind in PaneKind::all().into_iter().filter(|k| !k.online()) {
            let mut pane = kind.make();
            assert!(pane.poll_fetch().is_none(), "{} fetched", kind.name());
        }
        // The default (no `online`) build has no networked panes at all.
        #[cfg(not(feature = "online"))]
        assert!(PaneKind::all().iter().all(|k| !k.online()));
        // The online build adds networked panes to the registry.
        #[cfg(feature = "online")]
        assert!(PaneKind::all().iter().any(|k| k.online()));
    }
}
