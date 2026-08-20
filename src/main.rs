//! termling — a mood-driven doodle + arcade that lives *beside* your real
//! shell. Your shell stays entirely your own (its terminal, config, completions,
//! and prompt); a tiny zsh hook fires events at this companion, which renders the
//! creature and reacts. Entry point: dispatch subcommands, then set up the
//! terminal, run the event loop, and tear it down cleanly even on error.

mod app;
mod doodle;
mod emit;
mod event;
mod games;
mod init;
mod mood;
mod rng;
mod theme;
mod tmux;
mod ui;

use std::io::{self, Stdout};
use std::time::Instant;

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{
    event as term_event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, Mode};
use event::Receiver;
use games::Input;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Fired by the shell hook once per command — must be fast and silent.
        Some("emit") => emit::run(&args[2..]),
        // Prints the shell hook to `eval` from your rc file.
        Some("init") => init::run(&args[2..]),
        // Opens the companion in a tmux split (or prints the recipe).
        Some("tmux") => tmux::launch(),
        // Auto-spawn hook: spawn a companion for this scope unless one exists.
        Some("ensure") => tmux::ensure(),
        // Dump every mood's scene to stdout for art tweaking.
        Some("--preview") => {
            preview();
            Ok(())
        }
        // Default (and the explicit `sidebar` alias): the companion itself.
        _ => run_tui(),
    }
}

/// Set up the terminal, run the companion loop, and always tear down cleanly.
fn run_tui() -> io::Result<()> {
    // Bind the socket so we hear events from your shell.
    let receiver = match Receiver::bind() {
        Ok(rx) => Some(rx),
        // Another companion already owns this scope (e.g. two shells auto-spawned
        // at once) — step aside so we don't draw a second, deaf pane.
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => return Ok(()),
        // Bound failed for some other reason: run anyway, just without reacting.
        Err(_) => None,
    };
    let mut terminal = setup()?;
    let result = run(&mut terminal, App::new(), receiver);
    teardown(&mut terminal)?;
    result
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn teardown(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

fn run(terminal: &mut Tui, mut app: App, mut receiver: Option<Receiver>) -> io::Result<()> {
    let mut last_step = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Drain any events from your shell so the next frame reflects them.
        if let Some(rx) = receiver.as_mut() {
            for ev in rx.drain() {
                app.apply_event(ev);
            }
        }

        // Wait for input, but never past the next scheduled step, so the game
        // (and the doodle animation) advance on a steady cadence regardless of
        // whether keys or shell events are arriving.
        let interval = app.step_interval();
        let timeout = interval.saturating_sub(last_step.elapsed());
        if term_event::poll(timeout)? {
            if let TermEvent::Key(key) = term_event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, key);
                }
            }
        }
        if last_step.elapsed() >= interval {
            app.step();
            last_step = Instant::now();
        }
    }
    app.save();
    Ok(())
}

/// Dump the cast and every mood to stdout (no raw mode needed) so you can eyeball
/// the art. Colors won't show here — this is for shape/alignment checking.
fn preview() {
    use doodle::Character;
    use mood::Mood;
    let moods = [
        Mood::Sleepy,
        Mood::Bored,
        Mood::Curious,
        Mood::Focused,
        Mood::Toiling,
        Mood::Caffeinated,
        Mood::Happy,
        Mood::Proud,
        Mood::Excited,
        Mood::Confused,
    ];
    for &c in &Character::CAST {
        for m in moods {
            let d = doodle::render(m, c, 6);
            println!("== {} · {} ==  {}", d.name, m.label(), d.caption);
            for row in &d.rows {
                let text: String = row.iter().map(|s| s.text.as_str()).collect();
                println!("  |{text}|");
            }
            println!();
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Ctrl-C quits from anywhere.
    if ctrl && matches!(key.code, KeyCode::Char('c')) {
        app.should_quit = true;
        return;
    }

    match app.mode() {
        Mode::Ambient => ambient_key(app, key),
        Mode::Menu => menu_key(app, key),
        Mode::Game => game_key(app, key),
    }
}

/// The companion at rest: it reacts to your real shell, and a keypress wakes it
/// into the arcade. No text entry here — your real shell handles that.
fn ambient_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char('g') => app.open_menu(),
        KeyCode::Char('t') => app.cycle_theme(),
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        _ => {}
    }
}

fn menu_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Left | KeyCode::Char('k') | KeyCode::Char('h') => app.menu_prev(),
        KeyCode::Down | KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('l') => app.menu_next(),
        KeyCode::Enter | KeyCode::Char(' ') => app.menu_select(),
        KeyCode::Esc | KeyCode::Char('q') => app.exit_menu(),
        _ => {}
    }
}

fn game_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('h') => app.game_input(Input::Left),
        KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('l') => app.game_input(Input::Right),
        KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => app.game_input(Input::Up),
        KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') => app.game_input(Input::Down),
        KeyCode::Char(' ') => app.game_input(Input::Action),
        KeyCode::Char('r') => app.game_restart(),
        KeyCode::Esc | KeyCode::Char('q') => app.exit_game(),
        _ => {}
    }
}
