//! Terminal UI — main entry point and event loop.

pub mod app;
pub mod event;
pub mod text;
pub mod theme;
pub mod threading;
pub mod ui;
pub mod widgets;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{poll as ct_poll, read as ct_read, Event, KeyEventKind};
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
                // On Windows (and some terminals with kitty keyboard protocol),
                // crossterm emits both Press and Release events. Without this filter
                // every keystroke and pasted character would register twice.
                if key.kind == KeyEventKind::Press {
                    event::handle_key_event(&mut app, key)?;
                }
            }
        }

        // Collect results from any in-flight background search.
        app.poll_search();

        // Periodic housekeeping
        app.tick();

        // External HTML viewer requested? Suspend TUI, run, restore.
        if let Some(path) = app.pending_html_view.take() {
            run_external_html_viewer(terminal, &path, &mut app)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Suspend the alternate screen, run the configured HTML viewer on `path`,
/// then restore the TUI. Best-effort: any spawn/wait error is reported via
/// `app.set_status` and the loop continues.
fn run_external_html_viewer(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    path: &std::path::Path,
    app: &mut App,
) -> anyhow::Result<()> {
    let viewer = std::env::var("MBOXSHELL_HTML_VIEWER").unwrap_or_else(|_| "w3m".to_string());
    let mut parts = viewer.split_whitespace();
    let cmd = match parts.next() {
        Some(c) => c.to_string(),
        None => {
            app.set_status(i18n::tui_html_viewer_hint());
            return Ok(());
        }
    };
    let extra_args: Vec<String> = parts.map(str::to_string).collect();

    // Leave alternate screen so the viewer can use the real terminal.
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let status = std::process::Command::new(&cmd)
        .args(&extra_args)
        .arg(path)
        .status();

    // Restore TUI no matter what happened.
    enable_raw_mode()?;
    terminal.backend_mut().execute(EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    // Best-effort temp file cleanup.
    let _ = std::fs::remove_file(path);

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => app.set_status(&format!(
            "{}: {cmd} exited with {s}",
            i18n::tui_html_viewer_failed()
        )),
        Err(e) => app.set_status(&format!(
            "{}: {cmd}: {e} — {}",
            i18n::tui_html_viewer_failed(),
            i18n::tui_html_viewer_hint()
        )),
    }
    Ok(())
}
