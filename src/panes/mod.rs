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

mod buddy_insights;
mod git;
mod system;

use crate::doodle::{Row, Seg};
use crate::net::{FetchResult, FetchSpec};
use crate::theme::Theme;
use ratatui::style::Color;

#[cfg(feature = "online")]
mod devnews;
#[cfg(feature = "online")]
mod news;
#[cfg(feature = "online")]
mod stocks;
#[cfg(feature = "online")]
mod weather;
#[cfg(feature = "online")]
mod ai;

/// Every pane in the dashboard. Add a variant here (plus a file implementing
/// [`Pane`]) and wire it into [`ALL`](PaneKind::ALL), [`name`](PaneKind::name),
/// [`online`](PaneKind::online), and [`make`](PaneKind::make).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// A built-in welcome / help tile (always present).
    Welcome,
    /// A local git-repository summary for the current working dir.
    Git,
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

    /// Short sidebar label.
    pub fn name(self) -> &'static str {
        match self {
            PaneKind::Welcome => "welcome",
            PaneKind::Git => "git",
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

    /// A network request this pane wants made *now*, if its data is due. Local
    /// panes return `None`. The dashboard only forwards these in online mode.
    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        None
    }

    /// Fold a completed fetch (routed to this pane by kind) back into state.
    fn on_data(&mut self, _result: &FetchResult) {}
}

/// Build a one-line [`Row`] of a single color — the common case for text panes.
pub fn line(text: impl Into<String>, color: Color) -> Row {
    vec![Seg {
        text: text.into(),
        color,
    }]
}

/// The dashboard: the set of live panes, the sidebar selection, and whether we're
/// showing one focused pane or a grid of several. Lives on
/// [`App`](crate::app::App) like the arcade game does.
pub struct Dashboard {
    kinds: Vec<PaneKind>,
    panes: Vec<Box<dyn Pane>>,
    sel: usize,
    /// False → one focused pane fills the content area; true → a grid of panes.
    split: bool,
}

impl Dashboard {
    pub fn new() -> Dashboard {
        let kinds = PaneKind::all();
        let panes = kinds.iter().map(|k| k.make()).collect();
        Dashboard {
            kinds,
            panes,
            sel: 0,
            split: false,
        }
    }

    pub fn kinds(&self) -> &[PaneKind] {
        &self.kinds
    }

    pub fn sel(&self) -> usize {
        self.sel
    }

    pub fn split(&self) -> bool {
        self.split
    }

    pub fn toggle_split(&mut self) {
        self.split = !self.split;
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

    /// The currently-selected pane.
    pub fn focused(&self) -> Option<&dyn Pane> {
        self.panes.get(self.sel).map(|b| b.as_ref())
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
            line("s    toggle split grid", theme.dim),
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
        assert!(!dash.split());
        let pane = dash.focused().expect("a focused pane");
        assert_eq!(pane.title(), "welcome");
        let rows = pane.render(40, 12, &THEMES[0]);
        assert!(!rows.is_empty());
    }

    #[test]
    fn sidebar_navigation_wraps_and_split_toggles() {
        let mut dash = Dashboard::new();
        let n = dash.kinds().len();
        dash.prev(); // wrap backwards from 0
        assert_eq!(dash.sel(), n - 1);
        dash.next(); // back to 0
        assert_eq!(dash.sel(), 0);
        assert!(!dash.split());
        dash.toggle_split();
        assert!(dash.split());
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
