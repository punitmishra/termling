//! A scripted, self-running tour of the ambient companion — no shell, no socket.
//!
//! `termling demo` feeds a fixed timeline of synthetic shell events into a fresh
//! [`App`] so the creature parades through every mood, all three celebratory
//! flairs, and the multi-shell roster, then quits on its own after ~20s. It's
//! what records the README GIF (see `docs/demo.tape`) and doubles as a quick
//! manual sanity check of the whole render path.
//!
//! It deliberately points state at a throwaway dir and never calls `save`, so it
//! can't disturb your real XP/streak progress.

use std::io;
use std::time::Instant;

use crossterm::event::{self as term_event, Event as TermEvent, KeyCode, KeyEventKind};

use crate::app::App;
use crate::event::{Event, Kind};
use crate::ui;

/// Frame at which the demo ends and the process exits (~20s at the ~10fps
/// ambient cadence).
const END_FRAME: u64 = 205;

/// Build one synthetic event with a shell id and optional command / exit / cwd.
fn ev(kind: Kind, shell: &str, cmd: Option<&str>, code: Option<i32>, pwd: Option<&str>) -> Event {
    Event {
        kind,
        code,
        dur_ms: None,
        pwd: pwd.map(String::from),
        shell: Some(shell.to_string()),
        cmd: cmd.map(String::from),
    }
}

/// The scripted timeline: `(at_frame, event)`, strictly increasing by frame.
///
/// It's authored so each phase lands its intended mood/flair on camera (the
/// `replay_*` test enforces that): a long build (focused → toiling → a
/// celebrated finish), a run of successes (happy → excited, a streak milestone,
/// a level-up), a failure (confused) and recovery (proud), then a rapid-fire
/// burst (caffeinated). A second shell checks in mid-build so the roster shows
/// more than one.
fn timeline() -> Vec<(u64, Event)> {
    use Kind::{End, Start};
    let proj = Some("/Users/you/code/termling");
    vec![
        // A long release build: focused, then toiling past the ~4s threshold.
        (
            16,
            ev(Start, "%1", Some("cargo build --release"), None, proj),
        ),
        // A second shell finishes something mid-build → it joins the roster.
        (
            42,
            ev(End, "%2", Some("brew upgrade"), Some(0), Some("/Users/you")),
        ),
        // Build done after a long run → LongDone flair + terminal bell, then happy.
        (
            70,
            ev(End, "%1", Some("cargo build --release"), Some(0), None),
        ),
        // A run of quick successes: happy → excited, a milestone, a level-up.
        (96, ev(Start, "%1", Some("cargo test"), None, None)),
        (99, ev(End, "%1", Some("cargo test"), Some(0), None)),
        (108, ev(Start, "%1", Some("git status"), None, None)),
        (111, ev(End, "%1", Some("git status"), Some(0), None)),
        (118, ev(Start, "%1", Some("cargo clippy"), None, None)),
        (121, ev(End, "%1", Some("cargo clippy"), Some(0), None)),
        (128, ev(Start, "%1", Some("cargo fmt"), None, None)),
        (131, ev(End, "%1", Some("cargo fmt"), Some(0), None)),
        // A failure breaks the streak (confused), then a recovery (proud).
        (140, ev(Start, "%1", Some("npm run lint"), None, None)),
        (143, ev(End, "%1", Some("npm run lint"), Some(1), None)),
        (154, ev(Start, "%1", Some("npm run lint"), None, None)),
        (157, ev(End, "%1", Some("npm run lint"), Some(0), None)),
        // A rapid-fire burst tips it into caffeinated.
        (168, ev(End, "%1", Some("ls"), Some(0), None)),
        (169, ev(End, "%1", Some("cd src"), Some(0), None)),
        (170, ev(End, "%1", Some("cat main.rs"), Some(0), None)),
        (171, ev(End, "%1", Some("wc -l Cargo.toml"), Some(0), None)),
        (172, ev(End, "%1", Some("git diff"), Some(0), None)),
        (173, ev(End, "%1", Some("grep -r TODO"), Some(0), None)),
        (174, ev(End, "%1", Some("du -sh target"), Some(0), None)),
        (175, ev(End, "%1", Some("echo done"), Some(0), None)),
    ]
}

/// Run the scripted demo, restoring the terminal cleanly on exit. Press `q`,
/// `Esc`, or `Ctrl-C` to bail early.
pub fn run() -> io::Result<()> {
    // Never read or write the user's real progress: point state at a throwaway
    // dir (so `App::new` starts from zero) and skip `save` entirely.
    std::env::set_var(
        "XDG_STATE_HOME",
        std::env::temp_dir().join("termling-demo-state"),
    );

    let script = timeline();
    let mut app = App::new();
    let mut terminal = crate::setup()?;
    let result = drive(&mut terminal, &mut app, &script);
    crate::teardown(&mut terminal)?;
    result
}

/// The demo's event loop: apply any events due this frame, draw, and advance on
/// the same steady cadence the real companion uses.
fn drive(terminal: &mut crate::Tui, app: &mut App, script: &[(u64, Event)]) -> io::Result<()> {
    let mut next = 0usize;
    let mut last_step = Instant::now();

    while !app.should_quit && app.frame < END_FRAME {
        // Fire everything scheduled at or before the current frame.
        while next < script.len() && script[next].0 <= app.frame {
            app.apply_event(script[next].1.clone());
            next += 1;
        }
        terminal.draw(|f| ui::draw(f, app))?;

        let interval = app.step_interval();
        let timeout = interval.saturating_sub(last_step.elapsed());
        if term_event::poll(timeout)? {
            if let TermEvent::Key(key) = term_event::read()? {
                if key.kind == KeyEventKind::Press
                    && matches!(
                        key.code,
                        KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Esc
                    )
                {
                    app.should_quit = true;
                }
            }
        }
        if last_step.elapsed() >= interval {
            app.step();
            last_step = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Flair;
    use crate::mood::Mood;

    #[test]
    fn the_timeline_is_ordered_and_ends_before_the_deadline() {
        let t = timeline();
        assert!(!t.is_empty());
        assert!(
            t.windows(2).all(|w| w[0].0 <= w[1].0),
            "timeline must be sorted by frame"
        );
        assert!(t.last().unwrap().0 < END_FRAME);
    }

    /// Replaying the timeline through a fresh `App` must actually reach every
    /// headline mood and all three flairs — so the recorded GIF stays honest if
    /// the mood thresholds ever change.
    #[test]
    fn replay_parades_through_the_headline_moods_and_flairs() {
        // Sandbox persisted state (like `run` does) so `App::new` starts from
        // zero XP. This test asserts a specific flair *ordering* — the long
        // build must surface `LongDone`, not a `LevelUp` — which only holds from
        // a clean slate. Without this the test loads the developer's real
        // ~/.local/state/termling progress and fails once they've banked any XP,
        // even though it passes on a fresh CI machine.
        std::env::set_var(
            "XDG_STATE_HOME",
            std::env::temp_dir().join("termling-demo-replay-state-nonexistent"),
        );

        let script = timeline();
        let mut app = App::new();
        let mut next = 0usize;
        let mut moods: Vec<Mood> = Vec::new();
        let mut flairs: Vec<Flair> = Vec::new();

        while app.frame < END_FRAME {
            while next < script.len() && script[next].0 <= app.frame {
                app.apply_event(script[next].1.clone());
                next += 1;
            }
            let m = app.mood();
            if !moods.contains(&m) {
                moods.push(m);
            }
            if let Some(f) = app.flair() {
                if !flairs.contains(&f) {
                    flairs.push(f);
                }
            }
            app.step();
        }

        for m in [
            Mood::Curious,
            Mood::Focused,
            Mood::Toiling,
            Mood::Happy,
            Mood::Excited,
            Mood::Confused,
            Mood::Proud,
            Mood::Caffeinated,
        ] {
            assert!(moods.contains(&m), "demo never showed mood {m:?}");
        }
        for f in [Flair::LongDone, Flair::Milestone, Flair::LevelUp] {
            assert!(flairs.contains(&f), "demo never showed flair {f:?}");
        }

        // Two shells must appear in the roster (the multi-shell block).
        assert!(
            app.shells().len() >= 2,
            "demo should show more than one shell"
        );
    }
}
