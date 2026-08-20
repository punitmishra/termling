# termling

A little **animated, multi-color** ASCII creature that lives in a pane *beside
your shell* and reflects **its mood** based on how your commands go. It
breathes, blinks, waves, spins, and throws sparks in real time — over a
**mood-matched backdrop** — and a whole **cast of characters** takes turns on
stage as you work.

Crucially, **your shell stays entirely your own.** termling never wraps or
replaces it — no toy shell, nothing between you and zsh. Your terminal, your
config, your completions, your prompt, your aliases: all untouched. A tiny
`preexec`/`precmd` hook just whispers "a command started / finished with exit N"
to the companion pane, and the creature reacts.

Successful commands earn **XP** (streaks multiply it), you can switch **themes**
to suit your terminal, and there's a small **arcade** of mini-games for when a
build is running.

```
┌─ your zsh (100% yours) ────┐┌─termling · focused─┐
│ ~/proj ❯ git checkout <TAB>││       \ o /        │
│ checkout   cherry-pick     ││      ( ^   ^ )     │
│ ~/proj ❯ cargo test        ││      /|     |\     │
│ test result: ok. 60 passed ││  lv 4 ███░░░ 340xp │
│ ~/proj ❯ █                 ││  streak 3 · best 7 │
│                            ││  ✓ git             │
│                            ││  ✓ cargo           │
│                            ││ g play · aurora · q│
└────────────────────────────┘└────────────────────┘
   your real shell, with            the companion
   completions & prompt             (this program)
```

The left pane is **your own zsh** — termling isn't drawing it. The right
pane is this program.

## Quick start

**Install the hook** in your `~/.zshrc` (additive — it only registers
`preexec`/`precmd` and changes nothing else about your shell):

```sh
eval "$(termling init zsh)"
```

That's it. **The companion spawns itself** beside your shell the next time you
open one — a tmux split if you're in tmux, an iTerm2 split (via AppleScript) if
you're in iTerm, once per **scope** (see below) so several panes share one
creature instead of piling up. Just start working.

Every command makes the creature react — `focused` while it runs, `toiling` if
it drags on, then `happy`/`excited` on success or `confused` on failure — and a
small feed shows your recent commands with a `✓`/`✗`.

If you'd rather place it yourself (or you're not in tmux/iTerm), set
`TERMLING_AUTOSPAWN=0` and open it however you like:

```sh
termling tmux                              # splits a 34-col pane (or prints the recipe)
zellij --layout contrib/termling.kdl       # zellij
termling                                   # a plain second terminal window
```

Building from source:

```sh
cargo build --release      # binary at target/release/termling
cargo run                  # or just run the companion directly
```

The companion is **ambient** by default (it takes no input — your shell has the
keyboard). Focus its pane and press `g` to open the arcade, `t` to cycle themes,
`q` (or `Ctrl-C`) to quit.

To flip through the doodle frames without a socket or a pane:

```sh
cargo run -- --preview
```

## How the link works

```
  your zsh  ──(preexec/precmd hook)──▶  termling emit  ──datagram──▶  socket
                                                                             │
                              termling  ◀── non-blocking recv ──────────┘
                              (the companion pane)
```

The hook fires a **fire-and-forget datagram** at
`${XDG_RUNTIME_DIR:-<tmpdir>}/termling-<scope>.sock` on every command. If the
companion isn't running, the send fails instantly and is ignored — so **your
prompt never stalls**, whether or not the pane is open. The wire format is a
tiny `\x1f`-delimited `key=value` line: no JSON, no serde, trivial to emit from
zsh and parse in Rust.

Because nothing is ever piped *through* the companion, there is no way for it to
slow down, mangle, or lose your shell's output, completions, or history.

### Scopes & multiple shells

A **scope** is one companion's territory. All shells in the same scope share one
socket (`termling-<scope>.sock`) and one companion, which tracks each shell
as a **roster** — so opening five tmux panes gives you *one* creature that shows
all five, not five creatures. The scope is resolved identically by the hook and
the companion:

1. `TERMLING_SCOPE` — an explicit override.
2. inside **tmux** → the tmux *session* (one companion per session).
3. inside **iTerm2** → the iTerm *window*.
4. otherwise → `default`.

Auto-spawn uses a dependency-free single-instance guard: the companion's socket
`bind` *is* the lock (creating the socket file is atomic), so if two shells start
at once only one companion survives — the other detects the live socket and steps
aside. The ambient pane lists the live shells with a busy/idle marker, each
shell's directory, and its last command's result.

### Long commands & project awareness

- A command that runs a while shifts the creature to **toiling**; when it
  finishes, a little flourish fires and the terminal bell rings. Set
  `TERMLING_NOTIFY=1` for a macOS notification too — handy for a long build
  in another pane.
- The pane shows your current directory, and with `TERMLING_PROJECT_THEME=1`
  each project gets a stable auto-picked theme (off by default so it never fights
  the manual `t` key).
- Level-ups and streak milestones pop a brief confetti flourish.

### Environment knobs

| Variable | Effect |
|----------|--------|
| `TERMLING_PRIVACY` | `name` (default) / `full` / `none` — how much command text leaves the shell |
| `TERMLING_SCOPE` | override the companion's scope (see above) |
| `TERMLING_AUTOSPAWN` | `0` disables spawning the companion at shell startup |
| `TERMLING_NOTIFY` | `1` sends a macOS notification when a long command finishes |
| `TERMLING_PROJECT_THEME` | `1` auto-themes per project directory |

### Privacy

Commands can carry secrets (`export TOKEN=…`), so **by default only the command's
first word leaves your shell**. Tune it with `TERMLING_PRIVACY`:

| Value  | Sends |
|--------|-------|
| `name` | just the program name, e.g. `git` — the **default** |
| `full` | the whole command line |
| `none` | no command text at all — only exit codes and timing |

### Persistence

Progress is saved to `${XDG_STATE_HOME:-~/.local/state}/termling/state` — a
one-line `"<xp> <best_streak>"` (no serde) — and restored next launch.

## XP & levels

Every command your shell runs that exits `0` earns XP, and a run of successes
multiplies it (capped ×5), so a clean streak is worth chasing. XP rolls up into
levels, shown with a `█████░░░` bar alongside your current and best streak.
Playing a mini-game banks its score as XP too.

## Themes

A **theme** sets the *chrome* tone — borders, HUD text, the XP bar, and the
mini-games' items — without touching the characters (their colors are their
identity). Press `t` in the companion pane to cycle; the current theme's name is
shown on the footer line.

Bundled: `aurora` (default), `neon`, `sunset`, `matrix`, `mono`, `ocean`,
`ember`, `sakura`, `coffee`, `vaporwave`, `forest`. A theme is just six colors
and a name in `theme.rs` — add one to the `const` array and it joins the rotation.

## The arcade (mini-games)

Press `g` (or `Enter`) to open the picker (`↑/↓` to choose, `Enter` to play,
`Esc` back) — handy while a long build churns away in the other pane. Every game
stars your current character, is tinted by the active theme, speeds up with
level, and banks its score as XP when you leave. Shared controls: movement on
`←/→/↑/↓` (or `wasd`, `hjkl`), `Space` to act, `r` to retry after a game over,
`Esc`/`q` to step out.

| Game         | Goal |
|--------------|------|
| **doodle drop** | Catch **★** (+10) and **◆** gems (+30), dodge **●** bombs. Catches build a combo; a miss resets it; bombs cost a life (3 total). |
| **gap dive**    | Slide your glider to thread the single gap in each falling wall — and scoop the **◆** gem inside it. One mistake ends the dive; the gap narrows and walls arrive faster as you climb. |
| **coil**        | A snake with a temper. Eat `+` food and timed `*` gems to grow; every few bites the arena sprouts a `#` rock. Hit a wall, a rock, or your own tail and it's over. |
| **lane whack**  | Five holes, one mallet. Whack `O` moles and `*` gems with `Space`, but leave `X` bombs alone. A missed mole or a whacked bomb costs a life; fast hits chain a combo. |
| **doodle dash** | An endless runner. Cacti (`#`) scroll in along the ground — tap `Space`/`↑` to hop them. Clear one for a bonus; mistime a jump and the run ends. It scrolls faster the further you get. |
| **star blast**  | A fixed shooter. Slide your ship along the bottom and `Space` to fire straight up at descending invaders (`W`, gold `@`). Pop them for points; let one reach your row and you lose a life (3 total). |
| **muncher**     | A pellet chase. Steer through the maze gobbling `·` pellets; grab a power dot (`O`) and the ghost (`M`) turns edible for a bonus. Clear the board and it refills, faster each lap. Get caught and you lose a life. |
| **brick bounce**| A compact breakout. Slide your paddle to keep the ball in play and clear the brick wall (gold bricks pay extra). Clear it and a fresh wall drops in, faster each time. Drop the ball and you lose a life. |

The picker scrolls, so the list keeps working however many games you add — just
implement the `MiniGame` trait in `src/games/` and list a `GameKind` variant, and
the picker and HUD pick it up automatically.

## The mood engine

Every frame, `mood::Mood::from_signals` maps a few shell signals to one mood
(first matching rule wins):

| Mood          | When                                         |
|---------------|----------------------------------------------|
| `focused`     | a command is running                         |
| `toiling`     | a command has been running a long time       |
| `curious`     | fresh start, waiting for input               |
| `sleepy`      | idle a long time                             |
| `bored`       | idle a little while                          |
| `caffeinated` | commands firing in rapid succession          |
| `confused`    | last command exited non-zero                 |
| `proud`       | a success right after a failure (recovered!) |
| `excited`     | 3+ successes in a row                        |
| `happy`       | last command exited `0`                      |

## Characters × moods (the creative bit)

The two ideas are kept **orthogonal**, so they multiply instead of add:

- A **character** (`doodle::Character`) is a *silhouette + palette*: the head
  top, the body walls, the base line, and four RGB colors (body, accent, eyes,
  mouth). The cast — `blob`, `cat`, `robot`, `ghost`, `fox`, `comet`, `astra`,
  `mixtape`, `tubeboy`, `jelly`, `draco` — rotates over time and with each
  command (`App::character_seed`).
- A **mood** is an *expression*: the eyes, mouth, and the particles drifting
  around the creature, all animated by the clock (`app.frame`).
- A **scene** is the *backdrop*: sparse, gently-twinkling glyphs filling the sky
  band, the ground, and the side margins behind the creature. Each mood picks a
  fitting one — a starry **night sky**, a breezy **sunny meadow**, a crackling
  **electric buzz**, or cool **deep focus** rails.

`doodle::render(mood, character, frame)` composes them onto a fixed 20×6 canvas
as individually-colored segments — the character painted over its backdrop — so
any mood renders on any character in that character's colors. Add a **character**
(a palette + a few glyphs), a **mood** (one expression), or a **scene** and every
combination lights up for free.

## Layout

- `main.rs`  — subcommand dispatch + terminal setup + steady-cadence event loop
- `app.rs`   — state, mood signals, XP/streaks, the recent-command feed, the multi-shell roster, flair, persistence
- `event.rs` — the shell↔companion wire format, scopes, the datagram socket, liveness probe, and receiver
- `emit.rs`  — the `emit` subcommand the hook fires per command (+ privacy knob)
- `init.rs`  — the `init zsh` subcommand that prints the hook
- `tmux.rs`  — spawning the companion (`tmux`/`ensure`): tmux split, iTerm AppleScript, single-instance guard
- `mood.rs`  — signals → mood
- `doodle.rs`— characters + mood expressions + scenery → colored ASCII scene
- `games/`   — the arcade: a `MiniGame` trait plus one module per game
- `rng.rs`   — a tiny dependency-free xorshift PRNG
- `theme.rs` — the switchable chrome/game palettes
- `ui.rs`    — the single-pane ratatui layout (ambient / arcade / game)

Only two dependencies (`ratatui` + `crossterm`); the socket protocol, games,
RNG, and themes are all hand-rolled to keep it lightweight.

## Not yet

bash/fish hooks (zsh-only for now); auto-spawn beyond tmux + iTerm2 (Kitty,
WezTerm, Terminal.app); per-command mood nuance; and a `zsh/net/socket`
zero-spawn emit path.

## Contributing

Contributions are very welcome — the codebase is deliberately small and
hackable. Some good first PRs, roughly by area:

- **A new mini-game** — implement the `MiniGame` trait in `src/games/` and add a
  `GameKind` variant; the picker and HUD wire it in automatically.
- **A new character** — a silhouette + four RGB colors in `doodle.rs` (see the
  existing cast); it works across every mood for free.
- **A new theme** — six colors and a name in `theme.rs`.
- **A new mood or scene** — a rule in `mood.rs` plus an expression/backdrop in
  `doodle.rs`.
- **Another shell** — a `bash`/`fish` hook alongside the zsh one in `init.rs`.

To get set up:

```sh
git clone https://github.com/punitmishra/termling
cd termling
cargo run -- --preview     # eyeball the art without a socket or a pane
cargo test                 # the suite is fast and dependency-light
cargo clippy --all-targets # keep it warning-clean
```

Only two runtime dependencies (`ratatui` + `crossterm`) — please keep it that
way where you reasonably can; the sockets, RNG, games, and themes are all
hand-rolled on purpose. Run `cargo fmt`, `cargo clippy`, and `cargo test`
before opening a PR, and open an issue first for anything large so we can agree
on the shape.

## License

Released under the [MIT License](LICENSE) — free to use, modify, and
distribute. Contributions are accepted under the same license.
