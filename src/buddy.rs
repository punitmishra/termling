//! The buddy brain: a tiny, fully-local learner that watches the commands you
//! run and talks back in a speech bubble.
//!
//! No network, no model weights, no new crates — just two hand-rolled structures
//! over your command *tools* (the program names that already arrive on the wire):
//!
//!   * [`Habit`] — per-tool stats: how often you run it, how often it succeeds,
//!     and how long it usually takes. Lets the buddy say "that build was slow for
//!     you" or "`docker` again — third time today".
//!   * a Markov *bigram* table — which tool tends to follow which — so the buddy
//!     can anticipate ("after `cargo build` you usually reach for `cargo test`").
//!
//! State persists to a few KB next to the XP file (`termling/buddy`) in the same
//! hand-rolled, serde-free spirit as the rest of the crate, and is bounded so it
//! can never grow past a handful of KB however long you use it.
//!
//! Everything here is deliberately independent of *characters* and *moods*: the
//! buddy produces a [`Quip`] (a short line + a [`Tone`]), and the UI decides how
//! to draw it. Adding a quip or a habit lights up across the whole cast for free.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The emotional color of a quip — the UI maps it to a palette entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A success / encouragement.
    Cheer,
    /// A failure — gentle, never scolding.
    Worry,
    /// A learned observation about your habits or a prediction.
    Insight,
    /// Neutral chatter (greetings, idle musings).
    Neutral,
}

/// One thing the buddy wants to say, kept short enough for a speech bubble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quip {
    pub text: String,
    pub tone: Tone,
}

impl Quip {
    fn new(tone: Tone, text: impl Into<String>) -> Quip {
        Quip {
            text: text.into(),
            tone,
        }
    }
}

/// Per-tool running stats. Cheap to fold and cheap to serialize.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Habit {
    /// Times this tool has finished (any exit code).
    pub runs: u32,
    /// Of those, how many exited 0.
    pub ok: u32,
    /// Sum of measured durations (ms), over `dur_samples` samples.
    total_ms: u64,
    /// How many finishes carried a usable (>0ms) duration.
    dur_samples: u32,
}

impl Habit {
    /// Success rate in `0..=1`, or `None` before the first run.
    pub fn success_rate(&self) -> Option<f32> {
        (self.runs > 0).then(|| self.ok as f32 / self.runs as f32)
    }

    /// Typical duration in ms, once we've timed at least one run.
    pub fn typical_ms(&self) -> Option<u64> {
        (self.dur_samples > 0).then(|| self.total_ms / self.dur_samples as u64)
    }
}

/// The learned model of *your* workflow.
pub struct Buddy {
    /// Per-tool habit stats, keyed by the tool token (`git`, `cargo`, …).
    habits: BTreeMap<String, Habit>,
    /// Markov bigram counts: `from` tool → (`to` tool → times seen in sequence).
    next: BTreeMap<String, BTreeMap<String, u32>>,
    /// The last tool that *finished*, to draw the next bigram edge from.
    last_tool: Option<String>,
    /// Where state persists (None only in tests that opt out of disk).
    path: Option<PathBuf>,
}

/// Keep the model bounded no matter how long termling runs.
const MAX_TOOLS: usize = 256;
/// Cap on distinct successors remembered per tool.
const MAX_SUCCESSORS: usize = 32;
/// A bigram edge this strong (as a share of the source tool's total) is a
/// confident enough habit to predict out loud.
const PREDICT_CONFIDENCE: f32 = 0.5;
/// A tool must have been seen this many times before we'll predict from it.
const PREDICT_MIN_RUNS: u32 = 3;
/// A run this many times its own typical duration is "slow for you"; its
/// reciprocal is "snappy". Only remarked on past `SLOW_FLOOR_MS`.
const SLOW_FACTOR: f32 = 1.6;
const SLOW_FLOOR_MS: u64 = 2_000;

impl Buddy {
    /// Load the learned model from disk, or start fresh if there's none.
    pub fn load() -> Buddy {
        // Under `cargo test` the whole crate compiles in test config; keep the
        // buddy off disk there so unit tests stay hermetic and never read or write
        // the user's real state file.
        let path = if cfg!(test) { None } else { state_path() };
        let mut b = Buddy {
            habits: BTreeMap::new(),
            next: BTreeMap::new(),
            last_tool: None,
            path,
        };
        if let Some(text) = b
            .path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
        {
            b.parse(&text);
        }
        b
    }

    /// A model that never touches disk — for tests and previews.
    #[cfg(test)]
    fn ephemeral() -> Buddy {
        Buddy {
            habits: BTreeMap::new(),
            next: BTreeMap::new(),
            last_tool: None,
            path: None,
        }
    }

    /// How many tools the buddy has come to know.
    pub fn known_tools(&self) -> usize {
        self.habits.len()
    }

    /// The `n` tools you run most, strongest first (ties broken by name), as
    /// `(tool, habit)` borrows into the live model. A read-only view for the
    /// buddy-insights dashboard pane; pair each [`Habit`] with
    /// [`Habit::success_rate`] / [`Habit::runs`] to draw a leaderboard.
    pub fn top_tools(&self, n: usize) -> Vec<(&str, &Habit)> {
        let mut v: Vec<(&str, &Habit)> = self.habits.iter().map(|(k, h)| (k.as_str(), h)).collect();
        v.sort_by(|a, b| b.1.runs.cmp(&a.1.runs).then_with(|| a.0.cmp(b.0)));
        v.truncate(n);
        v
    }

    /// The tool the buddy expects to follow `from`, with confidence in `0..=1`,
    /// when it is confident enough to say so (`None` otherwise). A public,
    /// read-only window onto the learned bigram model for the dashboard.
    pub fn predict_after(&self, from: &str) -> Option<(String, f32)> {
        self.prediction_from(from)
    }

    /// A command just *started*. We don't learn from it yet (no outcome), but it's
    /// a good moment to greet a never-seen tool or predict what usually follows the
    /// last thing that ran. Returns `None` when the buddy has nothing worth saying.
    pub fn on_start(&mut self, cmd: Option<&str>, nonce: u64) -> Option<Quip> {
        let tool = tool_of(cmd?)?.to_string();
        if !self.habits.contains_key(&tool) {
            return Some(Quip::new(
                Tone::Neutral,
                format!("first time seeing `{tool}` 👀"),
            ));
        }
        // If this start confirms the habit we'd have predicted, say so — it makes
        // the buddy feel like it was reading over your shoulder.
        if let Some((pred, _)) = self.prediction() {
            if pred == tool {
                return Some(Quip::new(Tone::Insight, format!("knew it — `{tool}` 🔮")));
            }
        }
        // Otherwise, occasionally note a frequent tool's tally.
        let runs = self.habits.get(&tool).map(|h| h.runs).unwrap_or(0);
        if runs >= 5 && nonce.is_multiple_of(4) {
            return Some(Quip::new(
                Tone::Neutral,
                format!("`{tool}` again — #{} for us", runs + 1),
            ));
        }
        None
    }

    /// A command *finished*. This is where the buddy learns (habit stats + the
    /// bigram edge from the previous tool) and reacts to the outcome.
    pub fn on_end(
        &mut self,
        cmd: Option<&str>,
        code: i32,
        dur_ms: Option<u64>,
        nonce: u64,
    ) -> Option<Quip> {
        let tool = cmd.and_then(tool_of).map(str::to_string);

        if let Some(tool) = &tool {
            self.learn(tool, code, dur_ms);
        }

        let quip = self.react(tool.as_deref(), code, dur_ms, nonce);

        // Advance the Markov chain: the next command's bigram draws from this one.
        self.last_tool = tool;
        self.save();
        quip
    }

    /// Fold one finished command into the model (stats + bigram edge).
    fn learn(&mut self, tool: &str, code: i32, dur_ms: Option<u64>) {
        // Bigram edge from the previously-finished tool to this one.
        if let Some(prev) = self.last_tool.clone() {
            if prev != *tool {
                let succ = self.next.entry(prev).or_default();
                *succ.entry(tool.to_string()).or_insert(0) += 1;
                prune_successors(succ);
            }
        }

        let h = self.habits.entry(tool.to_string()).or_default();
        h.runs = h.runs.saturating_add(1);
        if code == 0 {
            h.ok = h.ok.saturating_add(1);
        }
        if let Some(ms) = dur_ms.filter(|ms| *ms > 0) {
            h.total_ms = h.total_ms.saturating_add(ms);
            h.dur_samples = h.dur_samples.saturating_add(1);
        }

        self.prune_tools();
    }

    /// Pick something to say about a finished command. Learning has already run,
    /// so `self.habits[tool]` reflects this run too.
    fn react(
        &self,
        tool: Option<&str>,
        code: i32,
        dur_ms: Option<u64>,
        nonce: u64,
    ) -> Option<Quip> {
        let Some(tool) = tool else {
            // No command text (privacy=none): still acknowledge the outcome.
            return Some(if code == 0 {
                Quip::new(Tone::Cheer, pick(&GENERIC_OK, nonce))
            } else {
                Quip::new(Tone::Worry, pick(&GENERIC_FAIL, nonce))
            });
        };

        if code != 0 {
            // A gentle, personalized nudge on failure. If this tool trips you up
            // more often than not, the buddy commiserates instead of cheerleading.
            let often_tricky = self
                .habits
                .get(tool)
                .is_some_and(|h| h.runs >= 4 && h.success_rate().is_some_and(|r| r < 0.5));
            let tail = if often_tricky {
                "a tricky one for you"
            } else {
                pick(&FAIL_TAIL, nonce)
            };
            return Some(Quip::new(
                Tone::Worry,
                format!("`{tool}` hit exit {code} — {tail}"),
            ));
        }

        // Success. Prefer a *learned* remark when we have one, else a cheer.
        if let Some(insight) = self.duration_remark(tool, dur_ms) {
            return Some(insight);
        }
        // Roughly every few successes, surface a prediction instead of a plain cheer.
        if nonce.is_multiple_of(3) {
            if let Some((succ, _)) = self.prediction_from(tool) {
                return Some(Quip::new(
                    Tone::Insight,
                    format!("`{tool}` ✓ — `{succ}` next?"),
                ));
            }
        }
        Some(Quip::new(
            Tone::Cheer,
            format!("`{tool}` {}", pick(&OK_TAIL, nonce)),
        ))
    }

    /// A remark comparing this run's duration to the tool's typical time, when the
    /// gap is big enough to be worth mentioning.
    fn duration_remark(&self, tool: &str, dur_ms: Option<u64>) -> Option<Quip> {
        let ms = dur_ms.filter(|ms| *ms >= SLOW_FLOOR_MS)?;
        let typical = self.habits.get(tool).and_then(Habit::typical_ms)?;
        if typical == 0 {
            return None;
        }
        let ratio = ms as f32 / typical as f32;
        if ratio >= SLOW_FACTOR {
            Some(Quip::new(
                Tone::Insight,
                format!("`{tool}` took {} — slow for you", human_ms(ms)),
            ))
        } else if ratio <= 1.0 / SLOW_FACTOR {
            Some(Quip::new(
                Tone::Cheer,
                format!("`{tool}` in {} — snappy ⚡", human_ms(ms)),
            ))
        } else {
            None
        }
    }

    /// The most likely next tool after the *last finished* one, if confident.
    fn prediction(&self) -> Option<(String, f32)> {
        self.prediction_from(self.last_tool.as_deref()?)
    }

    /// The most likely tool to follow `from`, gated on confidence and sample size.
    fn prediction_from(&self, from: &str) -> Option<(String, f32)> {
        if self.habits.get(from).map(|h| h.runs).unwrap_or(0) < PREDICT_MIN_RUNS {
            return None;
        }
        let succ = self.next.get(from)?;
        let total: u32 = succ.values().sum();
        if total == 0 {
            return None;
        }
        let (tool, &count) = succ.iter().max_by_key(|(_, c)| **c)?;
        let confidence = count as f32 / total as f32;
        (confidence >= PREDICT_CONFIDENCE).then(|| (tool.clone(), confidence))
    }

    // --- bounding -------------------------------------------------------------

    /// If we know too many tools, forget the least-used ones (and any bigram edges
    /// that mention them), so the file stays small.
    fn prune_tools(&mut self) {
        if self.habits.len() <= MAX_TOOLS {
            return;
        }
        let mut by_use: Vec<(String, u32)> = self
            .habits
            .iter()
            .map(|(k, h)| (k.clone(), h.runs))
            .collect();
        by_use.sort_by_key(|(_, runs)| *runs);
        let drop_n = self.habits.len() - MAX_TOOLS;
        let doomed: Vec<String> = by_use.into_iter().take(drop_n).map(|(k, _)| k).collect();
        for tool in doomed {
            self.habits.remove(&tool);
            self.next.remove(&tool);
            for succ in self.next.values_mut() {
                succ.remove(&tool);
            }
        }
    }

    // --- persistence ----------------------------------------------------------
    //
    // Plain text, one record per line — tool tokens never contain whitespace, so
    // space-delimited fields parse trivially and stay diff-friendly:
    //
    //   v1
    //   h <tool> <runs> <ok> <total_ms> <dur_samples>
    //   n <from> <to> <count>

    fn parse(&mut self, text: &str) {
        for line in text.lines() {
            let mut f = line.split_whitespace();
            match f.next() {
                Some("h") => {
                    if let (Some(tool), Some(runs), Some(ok), Some(tms), Some(ds)) =
                        (f.next(), f.next(), f.next(), f.next(), f.next())
                    {
                        self.habits.insert(
                            tool.to_string(),
                            Habit {
                                runs: runs.parse().unwrap_or(0),
                                ok: ok.parse().unwrap_or(0),
                                total_ms: tms.parse().unwrap_or(0),
                                dur_samples: ds.parse().unwrap_or(0),
                            },
                        );
                    }
                }
                Some("n") => {
                    if let (Some(from), Some(to), Some(count)) = (f.next(), f.next(), f.next()) {
                        self.next
                            .entry(from.to_string())
                            .or_default()
                            .insert(to.to_string(), count.parse().unwrap_or(0));
                    }
                }
                _ => {} // version line and anything unknown: ignore.
            }
        }
    }

    fn serialize(&self) -> String {
        let mut out = String::from("v1\n");
        for (tool, h) in &self.habits {
            out.push_str(&format!(
                "h {} {} {} {} {}\n",
                tool, h.runs, h.ok, h.total_ms, h.dur_samples
            ));
        }
        for (from, succ) in &self.next {
            for (to, count) in succ {
                out.push_str(&format!("n {from} {to} {count}\n"));
            }
        }
        out
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.serialize());
    }
}

/// Trim a successor map back to the most-seen `MAX_SUCCESSORS` edges.
fn prune_successors(succ: &mut BTreeMap<String, u32>) {
    if succ.len() <= MAX_SUCCESSORS {
        return;
    }
    let mut edges: Vec<(String, u32)> = succ.iter().map(|(k, v)| (k.clone(), *v)).collect();
    edges.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    let keep: std::collections::BTreeSet<String> = edges
        .into_iter()
        .take(MAX_SUCCESSORS)
        .map(|(k, _)| k)
        .collect();
    succ.retain(|k, _| keep.contains(k));
}

/// The tool token of a command line: its first whitespace word, skipping any
/// leading `VAR=value` assignments (mirrors the emit-side privacy filter, so it
/// works whether privacy is `name` or `full`).
fn tool_of(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().find(|w| !w.contains('='))
}

/// A compact human duration: `820ms`, `3.4s`, `2m10s`.
fn human_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f32 / 1_000.0)
    } else {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

/// Deterministically pick a phrasing so quips vary without an RNG dependency.
fn pick<'a>(bank: &[&'a str], nonce: u64) -> &'a str {
    bank[(nonce as usize) % bank.len()]
}

const OK_TAIL: [&str; 5] = ["✓", "✓ nice", "✓ clean", "✓ ship it", "done ✓"];
const FAIL_TAIL: [&str; 4] = [
    "shake it off",
    "you've got this",
    "let's look",
    "try again?",
];
const GENERIC_OK: [&str; 3] = ["clean run ✓", "nice ✓", "that worked ✓"];
const GENERIC_FAIL: [&str; 3] = [
    "that one stumbled",
    "hmm, nonzero exit",
    "not quite — retry?",
];

/// `${XDG_STATE_HOME:-~/.local/state}/termling/buddy` — beside the XP state file.
fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("termling").join("buddy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_extraction_skips_env_assignments() {
        assert_eq!(tool_of("git push origin main"), Some("git"));
        assert_eq!(tool_of("FOO=1 BAR=2 cargo test"), Some("cargo"));
        assert_eq!(tool_of("   "), None);
    }

    #[test]
    fn a_never_seen_tool_is_greeted_on_start() {
        let mut b = Buddy::ephemeral();
        let q = b.on_start(Some("docker"), 0).unwrap();
        assert_eq!(q.tone, Tone::Neutral);
        assert!(q.text.contains("docker"));
    }

    #[test]
    fn learning_accumulates_habit_stats() {
        let mut b = Buddy::ephemeral();
        b.on_end(Some("cargo build"), 0, Some(3_000), 0);
        b.on_end(Some("cargo build"), 0, Some(5_000), 1);
        b.on_end(Some("cargo build"), 1, None, 2);
        let h = b.habits.get("cargo").unwrap();
        assert_eq!(h.runs, 3);
        assert_eq!(h.ok, 2);
        assert_eq!(h.typical_ms(), Some(4_000)); // (3000+5000)/2
        assert!((h.success_rate().unwrap() - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_failure_gets_a_gentle_worried_quip() {
        let mut b = Buddy::ephemeral();
        let q = b.on_end(Some("cargo test"), 101, Some(500), 7).unwrap();
        assert_eq!(q.tone, Tone::Worry);
        assert!(q.text.contains("cargo"));
        assert!(q.text.contains("101"));
    }

    #[test]
    fn a_confident_sequence_is_predicted() {
        let mut b = Buddy::ephemeral();
        // Teach a strong build→test habit.
        for i in 0..4 {
            b.on_end(Some("cargo"), 0, None, i);
            b.on_end(Some("npm"), 0, None, i); // note: last_tool now npm... reset below
        }
        // Build a clean, dominant edge: cargo → git, repeated.
        let mut b = Buddy::ephemeral();
        for i in 0..5 {
            b.on_end(Some("cargo"), 0, None, i * 2);
            b.on_end(Some("git"), 0, None, i * 2 + 1);
        }
        let (succ, conf) = b
            .prediction_from("cargo")
            .expect("should predict from cargo");
        assert_eq!(succ, "git");
        assert!(conf >= PREDICT_CONFIDENCE);
    }

    #[test]
    fn a_slow_run_is_remarked_against_your_typical() {
        let mut b = Buddy::ephemeral();
        // Establish a ~3s typical for cargo.
        b.on_end(Some("cargo"), 0, Some(3_000), 0);
        b.on_end(Some("cargo"), 0, Some(3_000), 1);
        // Now a much slower run.
        let q = b.on_end(Some("cargo"), 0, Some(9_000), 2).unwrap();
        assert_eq!(q.tone, Tone::Insight);
        assert!(q.text.contains("slow"));
    }

    #[test]
    fn state_round_trips_through_text() {
        let mut b = Buddy::ephemeral();
        b.on_end(Some("cargo"), 0, Some(3_000), 0);
        b.on_end(Some("git"), 1, Some(200), 1); // also records cargo→git edge
        let text = b.serialize();

        let mut restored = Buddy::ephemeral();
        restored.parse(&text);
        assert_eq!(restored.habits.get("cargo"), b.habits.get("cargo"));
        assert_eq!(restored.habits.get("git"), b.habits.get("git"));
        assert_eq!(restored.serialize(), text);
    }

    #[test]
    fn successor_pruning_keeps_the_strongest_edges() {
        let mut succ = BTreeMap::new();
        for i in 0..(MAX_SUCCESSORS + 10) {
            succ.insert(format!("t{i}"), i as u32);
        }
        prune_successors(&mut succ);
        assert_eq!(succ.len(), MAX_SUCCESSORS);
        // The strongest edge (highest count) must survive.
        assert!(succ.contains_key(&format!("t{}", MAX_SUCCESSORS + 9)));
    }
}
