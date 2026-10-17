//! A local git-repository summary pane.
//!
//! Shows the current working dir's branch, staged/dirty counts, how far it is
//! ahead/behind its upstream, and the last few commit subjects. It learns all of
//! this by shelling out to the `git` binary via [`std::process::Command`] — a
//! plain process spawn, *not* a network call, so the zero-network promise holds.
//! Spawning `git` is cheap but not free, so the pane refreshes at most once every
//! [`REFRESH_TICKS`] ticks and renders a cached snapshot in between. Outside a git
//! repo (or with no `git` on `PATH`) it degrades to a friendly placeholder.

use std::process::Command;

use super::bg::Bg;
use crate::doodle::Row;
use crate::panes::line;
use crate::theme::Theme;

/// Re-run `git` at most this often (in dashboard ticks).
const REFRESH_TICKS: u64 = 30;
/// How many recent commit subjects to show.
const RECENT_COMMITS: usize = 3;

/// A parsed snapshot of the repo, or the fact that there isn't one.
#[derive(Clone, Default)]
struct GitInfo {
    /// False when the working dir isn't a git repo (or `git` is missing).
    is_repo: bool,
    /// Current branch, a short detached-HEAD sha, or "—".
    branch: String,
    /// Files with staged (index) changes.
    staged: usize,
    /// Files with unstaged / untracked changes.
    dirty: usize,
    /// Commits ahead of the upstream, if one is tracked.
    ahead: Option<u32>,
    /// Commits behind the upstream, if one is tracked.
    behind: Option<u32>,
    /// Up to [`RECENT_COMMITS`] most-recent commit subjects.
    commits: Vec<String>,
}

/// The git summary tile. Holds a cached [`GitInfo`]; re-collection runs on a
/// background thread (see [`Bg`]) so a large repo never stalls the render loop.
pub struct GitPane {
    info: GitInfo,
    /// Tick at which we may refresh again; `0` means "due now".
    next_refresh: u64,
    /// Off-thread re-collection; its result lands on a later tick.
    bg: Bg<GitInfo>,
}

impl GitPane {
    /// Build the pane and kick off the first collection off-thread, so building
    /// it (which `App::new` does eagerly for every pane) never blocks startup on
    /// `git`. `info` stays empty until the first result lands, so `render` shows
    /// its placeholder until then.
    pub fn new() -> GitPane {
        let bg = Bg::new();
        bg.trigger(collect);
        GitPane {
            info: GitInfo::default(),
            next_refresh: REFRESH_TICKS,
            bg,
        }
    }
}

impl Default for GitPane {
    fn default() -> Self {
        GitPane::new()
    }
}

impl super::Pane for GitPane {
    fn title(&self) -> String {
        "git".into()
    }

    fn tick(&mut self, frame: u64) {
        // Pull in a completed background collection, if one landed.
        if let Some(fresh) = self.bg.take() {
            self.info = fresh;
        }
        // Kick off the next collection off-thread when due.
        if frame >= self.next_refresh {
            self.bg.trigger(collect);
            self.next_refresh = frame.saturating_add(REFRESH_TICKS);
        }
    }

    fn refresh(&mut self) {
        // Force the next tick to recollect (off-thread; never blocks here).
        self.next_refresh = 0;
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let i = &self.info;
        if !i.is_repo {
            return vec![
                line("not a git repo", theme.dim),
                line("(or git unavailable)", theme.dim),
            ];
        }

        let mut rows = Vec::new();
        rows.push(vec![
            crate::doodle::Seg {
                text: "on ".into(),
                color: theme.dim,
            },
            crate::doodle::Seg {
                text: i.branch.clone(),
                color: theme.accent,
            },
        ]);

        // staged / dirty summary, colored by cleanliness.
        if i.staged == 0 && i.dirty == 0 {
            rows.push(line("working tree clean ✓", theme.good));
        } else {
            rows.push(vec![
                crate::doodle::Seg {
                    text: format!("{} staged", i.staged),
                    color: theme.gem,
                },
                crate::doodle::Seg {
                    text: "  ".into(),
                    color: theme.dim,
                },
                crate::doodle::Seg {
                    text: format!("{} dirty", i.dirty),
                    color: theme.bad,
                },
            ]);
        }

        // ahead / behind, only when an upstream is tracked.
        match (i.ahead, i.behind) {
            (Some(a), Some(b)) if a > 0 || b > 0 => {
                rows.push(line(format!("↑{a} ahead  ↓{b} behind"), theme.accent));
            }
            (Some(_), Some(_)) => {
                rows.push(line("up to date with upstream", theme.dim));
            }
            _ => {
                rows.push(line("no upstream tracked", theme.dim));
            }
        }

        rows.push(line("", theme.dim));
        rows.push(line("recent", theme.dim));
        if i.commits.is_empty() {
            rows.push(line("(no commits yet)", theme.dim));
        } else {
            for c in &i.commits {
                rows.push(vec![
                    crate::doodle::Seg {
                        text: "• ".into(),
                        color: theme.bar,
                    },
                    crate::doodle::Seg {
                        text: c.clone(),
                        color: theme.dim,
                    },
                ]);
            }
        }
        rows
    }
}

/// Run `git` with `args` in the current dir, returning trimmed stdout on a clean
/// exit (and `None` on any failure — missing binary, non-repo, non-zero status).
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Shell out to `git` a handful of times and fold the output into a [`GitInfo`].
fn collect() -> GitInfo {
    // The cheapest "is this a repo?" probe; everything else is gated on it.
    if git(&["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return GitInfo::default();
    }

    let branch = git(&["symbolic-ref", "--short", "-q", "HEAD"])
        .filter(|b| !b.is_empty())
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]).map(|s| format!("detached @ {s}")))
        .unwrap_or_else(|| "—".into());

    let (mut staged, mut dirty) = (0usize, 0usize);
    if let Some(status) = git(&["status", "--porcelain"]) {
        for lineb in status.lines() {
            let b = lineb.as_bytes();
            if b.len() < 2 {
                continue;
            }
            let (x, y) = (b[0], b[1]);
            if x != b' ' && x != b'?' {
                staged += 1;
            }
            if y != b' ' {
                dirty += 1;
            }
        }
    }

    // ahead/behind vs upstream: left is upstream-only (behind), right is HEAD-only
    // (ahead). Absent when there's no tracking branch.
    let (mut ahead, mut behind) = (None, None);
    if let Some(lr) = git(&["rev-list", "--left-right", "--count", "@{u}...HEAD"]) {
        let mut it = lr.split_whitespace();
        if let (Some(l), Some(r)) = (it.next(), it.next()) {
            behind = l.parse().ok();
            ahead = r.parse().ok();
        }
    }

    let commits = git(&[
        "log",
        &format!("-{RECENT_COMMITS}"),
        "--format=%s",
        "--no-color",
    ])
    .map(|s| s.lines().map(str::to_string).collect())
    .unwrap_or_default();

    GitInfo {
        is_repo: true,
        branch,
        staged,
        dirty,
        ahead,
        behind,
        commits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::Pane;
    use crate::theme::THEMES;

    #[test]
    fn renders_non_empty_without_panicking() {
        // Runs against whatever dir the test executes in (a git repo or not);
        // either branch of `render` must yield rows and never panic.
        let pane = GitPane::new();
        for theme in THEMES {
            let rows = pane.render(40, 12, theme);
            assert!(!rows.is_empty());
        }
    }

    #[test]
    fn non_repo_info_renders_a_placeholder() {
        let pane = GitPane {
            info: GitInfo::default(),
            next_refresh: REFRESH_TICKS,
            bg: Bg::new(),
        };
        let rows = pane.render(40, 12, &THEMES[0]);
        assert!(!rows.is_empty());
        assert_eq!(pane.title(), "git");
    }

    #[test]
    fn refresh_marks_due_now() {
        let mut pane = GitPane::new();
        pane.next_refresh = 9_999; // push the next refresh far out…
        Pane::refresh(&mut pane); // …then `r` forces it due immediately.
        assert_eq!(pane.next_refresh, 0);
    }

    #[test]
    fn ticking_refreshes_on_cadence_without_panicking() {
        let mut pane = GitPane::new();
        for f in 0..(REFRESH_TICKS * 2 + 1) {
            pane.tick(f);
        }
        let rows = pane.render(40, 12, &THEMES[0]);
        assert!(!rows.is_empty());
    }
}
