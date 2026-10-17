//! A window onto the buddy brain: what you run most, how often it succeeds, and
//! what the buddy expects next.
//!
//! The pane is self-contained. It owns its own [`Buddy`], loaded from the same
//! persisted model the live creature learns into, and reloads it every
//! [`RELOAD_TICKS`] ticks so the dashboard reflects fresh activity without sharing
//! mutable state with the rest of the app. All reads go through the small public
//! accessors on [`Buddy`] ([`Buddy::top_tools`] / [`Buddy::predict_after`]); this
//! pane never writes. Fully local — no network, no new deps.

use crate::buddy::Buddy;
use crate::doodle::{Row, Seg};
use crate::panes::line;
use crate::theme::Theme;

/// Reload the persisted model at most this often (in dashboard ticks).
const RELOAD_TICKS: u64 = 30;
/// How many tools to list in the leaderboard.
const TOP_N: usize = 5;

/// The buddy-insights tile. Holds its own freshly-loaded [`Buddy`].
pub struct BuddyPane {
    buddy: Buddy,
    next_reload: u64,
}

impl BuddyPane {
    /// Build the pane, loading the buddy's persisted model immediately.
    pub fn new() -> BuddyPane {
        BuddyPane {
            buddy: Buddy::load(),
            next_reload: RELOAD_TICKS,
        }
    }
}

impl Default for BuddyPane {
    fn default() -> Self {
        BuddyPane::new()
    }
}

impl super::Pane for BuddyPane {
    fn title(&self) -> String {
        "buddy".into()
    }

    fn tick(&mut self, frame: u64) {
        if frame >= self.next_reload {
            self.buddy = Buddy::load();
            self.next_reload = frame.saturating_add(RELOAD_TICKS);
        }
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let top = self.buddy.top_tools(TOP_N);
        if top.is_empty() {
            return vec![
                line("no habits learned yet", theme.dim),
                line("run some commands and", theme.dim),
                line("the buddy will catch on", theme.dim),
            ];
        }

        let mut rows = vec![line(
            format!("{} tools known", self.buddy.known_tools()),
            theme.dim,
        )];

        for (tool, habit) in &top {
            // Color the success rate by how well it goes: good / gem / bad.
            let (rate_txt, rate_col) = match habit.success_rate() {
                Some(r) => {
                    let col = if r >= 0.8 {
                        theme.good
                    } else if r >= 0.5 {
                        theme.gem
                    } else {
                        theme.bad
                    };
                    (format!("{:>3.0}%", r * 100.0), col)
                }
                None => ("  —".to_string(), theme.dim),
            };
            rows.push(vec![
                Seg {
                    text: format!("{tool:<10} "),
                    color: theme.accent,
                },
                Seg {
                    text: rate_txt,
                    color: rate_col,
                },
                Seg {
                    text: format!("  ×{}", habit.runs),
                    color: theme.dim,
                },
            ]);
        }

        // Current prediction: what usually follows your most-run tool.
        rows.push(line("", theme.dim));
        let leader = top[0].0;
        match self.buddy.predict_after(leader) {
            Some((next, conf)) => rows.push(vec![
                Seg {
                    text: "next: ".into(),
                    color: theme.dim,
                },
                Seg {
                    text: next,
                    color: theme.accent,
                },
                Seg {
                    text: format!(" ({:.0}%)", conf * 100.0),
                    color: theme.dim,
                },
            ]),
            None => rows.push(line("no confident prediction yet", theme.dim)),
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::Pane;
    use crate::theme::THEMES;

    #[test]
    fn renders_non_empty_without_panicking() {
        // Under `cfg(test)` `Buddy::load()` is hermetic (no disk), so the model is
        // empty here — the pane must still render its placeholder and never panic.
        let pane = BuddyPane::new();
        for theme in THEMES {
            let rows = pane.render(40, 12, theme);
            assert!(!rows.is_empty());
        }
        assert_eq!(pane.title(), "buddy");
    }

    #[test]
    fn ticking_reloads_on_cadence_without_panicking() {
        let mut pane = BuddyPane::new();
        for f in 0..(RELOAD_TICKS * 2 + 1) {
            pane.tick(f);
        }
        assert!(!pane.render(40, 12, &THEMES[0]).is_empty());
    }
}
