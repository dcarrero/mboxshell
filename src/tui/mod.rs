//! Terminal UI — main entry point and event loop.

pub mod app;
pub mod event;
pub mod theme;
pub mod threading;
pub mod ui;
pub mod widgets;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{poll as ct_poll, read as ct_read, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use indicatif::{ProgressBar, ProgressStyle};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use self::app::App;
use crate::i18n;

/// Run the TUI application. Blocks until the user quits.
pub fn run_tui(mbox_path: PathBuf, force_reindex: bool) -> anyhow::Result<()> {
    // Show progress bar BEFORE entering alternate screen so the user sees it
    let file_size = std::fs::metadata(&mbox_path)?.len();
    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {} [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{eta}})",
                i18n::msg_indexing()
            ))
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let app = App::new_with_progress(mbox_path, force_reindex, &|current, total| {
        pb.set_length(total);
        pb.set_position(current);
    })?;

    pb.finish_and_clear();

    // Setup terminal (alternate screen)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop
    let result = run_event_loop(&mut terminal, app);

    // Restore terminal (always, even on error)
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main event loop: render → poll → handle → repeat.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
) -> anyhow::Result<()> {
    let tick_rate = Duration::from_millis(100);

    loop {
        // Render
        terminal.draw(|frame| {
            ui::render(frame, &mut app);
        })?;

        // Poll for events
        if ct_poll(tick_rate)? {
            if let Event::Key(key) = ct_read()? {
                event::handle_key_event(&mut app, key)?;
            }
        }

        // Periodic housekeeping
        app.tick();

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
