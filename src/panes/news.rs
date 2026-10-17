//! Top stories from the [Hacker News](https://github.com/HackerNews/API) API.
//!
//! The HN API needs two hops: `topstories.json` returns an ordered array of
//! item ids, then `item/<id>.json` returns each story. [`crate::net`] routes a
//! [`FetchResult`](crate::net::FetchResult) back by [`PaneKind`] only — it does
//! not echo the URL — so [`on_data`](Pane::on_data) tells the two payloads apart
//! by *shape*: a bare JSON array is the id list, a JSON object is one item.
//!
//! A refresh cycle therefore issues one request per tick: first the id list,
//! then the top handful of items in turn. The dashboard ticks ~10×/second, so a
//! full cycle settles in well under a second and then idles until stale.

use std::collections::VecDeque;

use serde::Deserialize;

use super::{line, Pane, PaneKind};
use crate::doodle::{Row, Seg};
use crate::net::{self, FetchResult, FetchSpec};
use crate::theme::Theme;

/// How many headlines to show.
const WANT: usize = 6;

/// Frames between full refreshes (~10 min at ~10 ticks/second).
const REFRESH_FRAMES: u64 = 10 * 60 * 10;

const TOP_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";

pub struct News {
    frame: u64,
    /// Frame the current cycle's top-list fetch began.
    fetched_at: Option<u64>,
    /// A cycle (top list, then items) is in progress.
    cycle_active: bool,
    /// We've received the id list for the current cycle.
    have_top: bool,
    /// A single request is outstanding.
    pending: bool,
    /// Item ids still to fetch this cycle.
    queue: VecDeque<u64>,
    /// Completed headlines, in arrival (rank) order.
    stories: Vec<String>,
    error: Option<String>,
}

impl News {
    pub fn new() -> News {
        News {
            frame: 0,
            fetched_at: None,
            cycle_active: false,
            have_top: false,
            pending: false,
            queue: VecDeque::new(),
            stories: Vec::new(),
            error: None,
        }
    }

    fn stale(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(f) => self.frame.saturating_sub(f) >= REFRESH_FRAMES,
        }
    }
}

impl Pane for News {
    fn title(&self) -> String {
        "hacker news".into()
    }

    fn tick(&mut self, frame: u64) {
        self.frame = frame;
    }

    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        if !net::online_enabled() || self.pending {
            return None;
        }
        // Start a fresh cycle when the cached list has gone stale.
        if !self.cycle_active && self.stale() {
            self.cycle_active = true;
            self.have_top = false;
        }
        if !self.cycle_active {
            return None;
        }
        if !self.have_top {
            self.pending = true;
            self.fetched_at = Some(self.frame);
            return Some(FetchSpec::get(PaneKind::News, TOP_URL));
        }
        if let Some(id) = self.queue.pop_front() {
            self.pending = true;
            return Some(FetchSpec::get(
                PaneKind::News,
                format!("https://hacker-news.firebaseio.com/v0/item/{id}.json"),
            ));
        }
        // Nothing left to fetch — the cycle is complete.
        self.cycle_active = false;
        None
    }

    fn on_data(&mut self, result: &FetchResult) {
        self.pending = false;
        let body = match &result.body {
            Err(e) => {
                self.error = Some(short(e));
                self.cycle_active = false;
                return;
            }
            Ok(b) => b,
        };
        // Shape-based routing: an array is the id list; an object is one item.
        if let Ok(ids) = serde_json::from_str::<Vec<u64>>(body) {
            self.have_top = true;
            self.stories.clear();
            self.queue = ids.into_iter().take(WANT).collect();
            self.error = None;
        } else if let Ok(item) = serde_json::from_str::<HnItem>(body) {
            if let Some(title) = item.title {
                self.stories.push(title);
            }
            self.error = None;
        } else {
            self.error = Some("bad response".into());
            self.cycle_active = false;
        }
    }

    fn render(&self, width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Vec::new();
        if !self.stories.is_empty() {
            let max = (width as usize).saturating_sub(3).max(8);
            for (i, title) in self.stories.iter().enumerate() {
                rows.push(vec![
                    Seg {
                        text: format!("{}. ", i + 1),
                        color: theme.dim,
                    },
                    Seg {
                        text: clip(title, max),
                        color: theme.accent,
                    },
                ]);
            }
        } else if let Some(e) = &self.error {
            rows.push(line("couldn't fetch:", theme.bad));
            rows.push(line(e.clone(), theme.dim));
        } else if !net::online_enabled() {
            rows.push(line("offline", theme.dim));
            rows.push(line("set TERMLING_ONLINE=1", theme.dim));
        } else {
            rows.push(line("loading…", theme.dim));
        }
        rows
    }
}

#[derive(Deserialize)]
struct HnItem {
    #[serde(default)]
    title: Option<String>,
}

/// Clip a title to `max` columns (by char), adding an ellipsis when cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let take = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(take).collect();
    out.push('…');
    out
}

fn short(e: &str) -> String {
    let first = e.trim().lines().next().unwrap_or("").trim();
    first.chars().take(56).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    const TOP_SAMPLE: &str = "[38000,38001,38002,38003,38004,38005,38006,38007]";
    const ITEM_SAMPLE: &str = r#"{"by":"pg","id":38000,"score":256,
        "title":"Show HN: A tiny terminal creature","type":"story",
        "url":"https://example.com"}"#;

    fn text(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn top_list_queues_at_most_want_items() {
        let mut n = News::new();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(TOP_SAMPLE.into()),
        });
        assert!(n.have_top);
        assert_eq!(n.queue.len(), WANT);
        assert_eq!(n.queue.front().copied(), Some(38000));
    }

    #[test]
    fn item_payload_appends_a_headline() {
        let mut n = News::new();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(ITEM_SAMPLE.into()),
        });
        let out = text(&n.render(40, 12, &THEMES[0]));
        assert!(out.contains("Show HN: A tiny terminal creature"), "{out}");
        assert!(out.contains("1. "), "ranked: {out}");
    }

    #[test]
    fn long_titles_are_clipped_to_width() {
        let long = "x".repeat(200);
        assert!(clip(&long, 20).chars().count() <= 20);
        assert!(clip(&long, 20).ends_with('…'));
        assert_eq!(clip("short", 20), "short");
    }

    #[test]
    fn errors_surface() {
        let mut n = News::new();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Err("timed out".into()),
        });
        let out = text(&n.render(40, 12, &THEMES[0]));
        assert!(out.contains("timed out"), "{out}");
    }
}
