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

use super::{clip, line, news_count, short, Pane, PaneKind, RefreshTimer};
use crate::doodle::{Row, Seg};
use crate::net::{self, FetchResult, FetchSpec};
use crate::theme::Theme;

/// Frames between full refreshes (~10 min at ~10 ticks/second).
const REFRESH_FRAMES: u64 = 10 * 60 * 10;

const TOP_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";

pub struct News {
    /// How many headlines to show (`TERMLING_NEWS_COUNT`, clamped 1..=12).
    want: usize,
    /// Staleness clock: when to start a fresh fetch cycle.
    timer: RefreshTimer,
    /// A cycle (top list, then items) is in progress.
    cycle_active: bool,
    /// We've received the id list for the current cycle.
    have_top: bool,
    /// A single request is outstanding.
    pending: bool,
    /// Item ids still to fetch this cycle.
    queue: VecDeque<u64>,
    /// Headlines being gathered for the in-progress cycle; swapped into
    /// `stories` only once the cycle completes, so a refresh never blanks.
    staging: Vec<String>,
    /// Completed headlines, in arrival (rank) order.
    stories: Vec<String>,
    error: Option<String>,
}

impl News {
    pub fn new() -> News {
        News {
            want: news_count(),
            timer: RefreshTimer::new(REFRESH_FRAMES),
            cycle_active: false,
            have_top: false,
            pending: false,
            queue: VecDeque::new(),
            staging: Vec::new(),
            stories: Vec::new(),
            error: None,
        }
    }

    fn stale(&self) -> bool {
        self.timer.due()
    }
}

impl Pane for News {
    fn title(&self) -> String {
        "hacker news".into()
    }

    fn tick(&mut self, frame: u64) {
        self.timer.tick(frame);
    }

    fn refresh(&mut self) {
        // Force a fresh cycle on the next poll by marking the cache stale.
        self.timer.reset();
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
            self.timer.mark();
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
            // Gather the new set off to the side; the currently-shown `stories`
            // stay on screen until this cycle finishes.
            self.staging.clear();
            self.queue = ids.into_iter().take(self.want).collect();
            self.error = None;
        } else if let Ok(item) = serde_json::from_str::<HnItem>(body) {
            if let Some(title) = item.title {
                self.staging.push(title);
            }
            self.error = None;
        } else if self.have_top {
            // HN returns `null` for dead/deleted items. Once we're fetching items
            // (`have_top`), skip an unparseable/null one and keep the queue going
            // rather than aborting the whole cycle with "bad response".
        } else {
            self.error = Some("bad response".into());
            self.cycle_active = false;
        }
        // The queue drains (in `poll_fetch`) as items are requested, so an empty
        // queue after an item means the fresh set is complete — swap it in at
        // once, replacing the old headlines in place with no "loading…" gap.
        if self.have_top && self.queue.is_empty() && !self.staging.is_empty() {
            self.stories = std::mem::take(&mut self.staging);
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
        // The sample top list has 8 ids; the default count (6) caps the queue.
        assert_eq!(n.queue.len(), n.want);
        assert_eq!(n.queue.front().copied(), Some(38000));
    }

    #[test]
    fn news_count_env_clamps_to_range() {
        use crate::panes::news_count;
        std::env::set_var("TERMLING_NEWS_COUNT", "3");
        assert_eq!(news_count(), 3);
        std::env::set_var("TERMLING_NEWS_COUNT", "99"); // over the max
        assert_eq!(news_count(), 12);
        std::env::set_var("TERMLING_NEWS_COUNT", "0"); // under the min
        assert_eq!(news_count(), 1);
        std::env::set_var("TERMLING_NEWS_COUNT", "junk"); // unparseable → default
        assert_eq!(news_count(), 6);
        std::env::remove_var("TERMLING_NEWS_COUNT");
        assert_eq!(news_count(), 6); // unset → default
    }

    #[test]
    fn item_payload_appends_a_headline() {
        let mut n = News::new();
        // Drive a minimal cycle: a one-id top list, the dashboard dispatching
        // that id (which drains the queue), then the item itself. Completing the
        // cycle swaps the staged headline into view.
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok("[38000]".into()),
        });
        n.queue.pop_front(); // poll_fetch would pop this id to request the item
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(ITEM_SAMPLE.into()),
        });
        let out = text(&n.render(40, 12, &THEMES[0]));
        assert!(out.contains("Show HN: A tiny terminal creature"), "{out}");
        assert!(out.contains("1. "), "ranked: {out}");
    }

    #[test]
    fn refresh_keeps_old_headlines_until_new_set_is_ready() {
        let mut n = News::new();
        // First cycle lands one headline.
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok("[1]".into()),
        });
        n.queue.pop_front();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(r#"{"id":1,"title":"old story"}"#.into()),
        });
        assert_eq!(n.stories, vec!["old story".to_string()]);

        // A refresh cycle begins: the new top list arrives, but the old headline
        // must stay on screen (no blank "loading…") until the new one is ready.
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok("[2]".into()),
        });
        let mid = text(&n.render(40, 12, &THEMES[0]));
        assert!(
            mid.contains("old story"),
            "old headline dropped early: {mid}"
        );
        assert!(!mid.contains("loading"), "blanked on refresh: {mid}");

        // The new item completes the cycle and swaps in place.
        n.queue.pop_front();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(r#"{"id":2,"title":"new story"}"#.into()),
        });
        assert_eq!(n.stories, vec!["new story".to_string()]);
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

    #[test]
    fn null_item_is_skipped_not_fatal() {
        // HN returns literal `null` for a dead/deleted item; it must not abort
        // the cycle. Prime the item-fetch phase with a top list first.
        let mut n = News::new();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(TOP_SAMPLE.into()),
        });
        assert!(n.have_top);
        let before = n.staging.len();
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok("null".into()),
        });
        // Skipped: no error, no phantom headline staged.
        assert!(n.error.is_none(), "null should not raise an error");
        assert_eq!(n.staging.len(), before);
        // A good item after the null still stages normally (the cycle still has
        // queued ids, so nothing has swapped into `stories` yet).
        n.on_data(&FetchResult {
            pane: PaneKind::News,
            body: Ok(ITEM_SAMPLE.into()),
        });
        assert_eq!(n.staging.len(), before + 1);
    }

    #[test]
    fn refresh_marks_stale() {
        let mut n = News::new();
        n.timer.tick(1);
        n.timer.mark(); // just fetched → not stale
        assert!(!n.stale());
        n.refresh();
        assert!(n.stale()); // `r` forces a fresh cycle
    }
}
