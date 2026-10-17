//! The hottest stories from [Lobsters](https://lobste.rs/), a computing-focused
//! link aggregator.
//!
//! One hop: `hottest.json` returns an array of story objects; we keep the top
//! handful of titles with their scores. Single fetch, refreshed every few
//! minutes — the same non-blocking [`poll_fetch`](Pane::poll_fetch) /
//! [`on_data`](Pane::on_data) dance as the other online panes.

use serde::Deserialize;

use super::{clip, line, news_count, short, Pane, PaneKind};
use crate::doodle::{Row, Seg};
use crate::net::{self, FetchResult, FetchSpec};
use crate::theme::Theme;

/// Frames between refreshes (~10 min at ~10 ticks/second).
const REFRESH_FRAMES: u64 = 10 * 60 * 10;

const URL: &str = "https://lobste.rs/hottest.json";

struct Story {
    title: String,
    score: i64,
}

pub struct DevNews {
    /// How many stories to show (`TERMLING_NEWS_COUNT`, clamped 1..=12).
    want: usize,
    frame: u64,
    fetched_at: Option<u64>,
    pending: bool,
    stories: Vec<Story>,
    error: Option<String>,
}

impl DevNews {
    pub fn new() -> DevNews {
        DevNews {
            want: news_count(),
            frame: 0,
            fetched_at: None,
            pending: false,
            stories: Vec::new(),
            error: None,
        }
    }

    fn due(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(f) => self.frame.saturating_sub(f) >= REFRESH_FRAMES,
        }
    }
}

impl Pane for DevNews {
    fn title(&self) -> String {
        "lobsters".into()
    }

    fn tick(&mut self, frame: u64) {
        self.frame = frame;
    }

    fn refresh(&mut self) {
        // Mark the cached stories stale so the next poll re-fetches.
        self.fetched_at = None;
    }

    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        if !net::online_enabled() || self.pending || !self.due() {
            return None;
        }
        self.pending = true;
        self.fetched_at = Some(self.frame);
        Some(FetchSpec::get(PaneKind::DevNews, URL))
    }

    fn on_data(&mut self, result: &FetchResult) {
        self.pending = false;
        match &result.body {
            Err(e) => self.error = Some(short(e)),
            Ok(body) => match parse(body, self.want) {
                Ok(s) if s.is_empty() => self.error = Some("no stories".into()),
                Ok(s) => {
                    self.stories = s;
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            },
        }
    }

    fn render(&self, width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Vec::new();
        if !self.stories.is_empty() {
            for s in &self.stories {
                let score = format!("{:>3} ", s.score);
                let max = (width as usize)
                    .saturating_sub(score.chars().count() + 1)
                    .max(8);
                rows.push(vec![
                    Seg {
                        text: score,
                        color: theme.good,
                    },
                    Seg {
                        text: clip(&s.title, max),
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
struct LobStory {
    #[serde(default)]
    title: String,
    #[serde(default)]
    score: i64,
}

fn parse(body: &str, want: usize) -> Result<Vec<Story>, String> {
    let raw: Vec<LobStory> = serde_json::from_str(body).map_err(|e| short(&e.to_string()))?;
    Ok(raw
        .into_iter()
        .filter(|s| !s.title.is_empty())
        .take(want)
        .map(|s| Story {
            title: s.title,
            score: s.score,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    const SAMPLE: &str = r#"[
        {"title":"Rust 2.0 announced","score":142,"comment_count":88,"url":"https://a"},
        {"title":"A new terminal UI toolkit","score":57,"url":"https://b"},
        {"title":"","score":9}
    ]"#;

    fn text(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parses_titles_and_scores_skipping_blank() {
        let s = parse(SAMPLE, 6).expect("parse");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "Rust 2.0 announced");
        assert_eq!(s[0].score, 142);
        // The count caps the list: ask for one and only the top story survives.
        assert_eq!(parse(SAMPLE, 1).expect("parse").len(), 1);
    }

    #[test]
    fn renders_scores_and_titles() {
        let mut d = DevNews::new();
        d.on_data(&FetchResult {
            pane: PaneKind::DevNews,
            body: Ok(SAMPLE.into()),
        });
        let out = text(&d.render(50, 12, &THEMES[0]));
        assert!(out.contains("142"), "score: {out}");
        assert!(out.contains("Rust 2.0 announced"), "title: {out}");
    }

    #[test]
    fn errors_surface() {
        let mut d = DevNews::new();
        d.on_data(&FetchResult {
            pane: PaneKind::DevNews,
            body: Err("connection refused".into()),
        });
        let out = text(&d.render(50, 12, &THEMES[0]));
        assert!(out.contains("connection refused"), "{out}");
    }
}
