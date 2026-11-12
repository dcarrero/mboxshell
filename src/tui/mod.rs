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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

/// True while the TUI owns the terminal (alternate screen).
///
/// Anything written to stderr in that window lands on top of the drawn
/// screen and stays there until the next full redraw, so the CLI's stderr
/// log layer checks this and stays quiet; the log file still gets everything.
pub static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

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

    // If anything panics while the alternate screen is active, restore the
    // terminal first so the user's shell is not left in raw mode (the normal
    // restore below is skipped when unwinding). Only for the main thread: a
    // panic in a search or export worker ends that task, not the interface,
    // which must keep its screen — the worker's failure is reported in the
    // status line and written to the log.
    let main_thread = std::thread::current().id();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == main_thread {
            let _ = disable_raw_mode();
            let _ = io::stdout().execute(LeaveAlternateScreen);
            default_hook(info);
        } else {
            tracing::error!("background task panicked: {info}");
        }
    }));

    // A terminal closed, a `kill`, a logout: leave through the normal path
    // instead of dying with the terminal still in raw mode on the alternate
    // screen. Registered only now, so indexing above stays interruptible.
    let terminate = Arc::new(AtomicBool::new(false));
    let signal_ids = register_termination_signals(&terminate);

    // Setup terminal (alternate screen)
    TUI_ACTIVE.store(true, Ordering::Relaxed);
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop
    let result = run_event_loop(&mut terminal, app, &terminate);
    TUI_ACTIVE.store(false, Ordering::Relaxed);

    // Restore the pre-TUI panic hook now that the alternate screen is going away.
    let _ = std::panic::take_hook();

    // Restore terminal (always, even on error)
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    for id in signal_ids {
        signal_hook::low_level::unregister(id);
    }

    result
}

/// Make SIGTERM, SIGINT and (on Unix) SIGHUP set `flag` instead of killing
/// the process. Best-effort: a signal that cannot be registered keeps its
/// default behaviour.
fn register_termination_signals(flag: &Arc<AtomicBool>) -> Vec<signal_hook::SigId> {
    let mut signals = vec![signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT];
    #[cfg(unix)]
    signals.push(signal_hook::consts::SIGHUP);
    signals
        .into_iter()
        .filter_map(|signal| signal_hook::flag::register(signal, Arc::clone(flag)).ok())
        .collect()
}

/// Most queued input events handled before the screen is drawn again.
const MAX_EVENTS_PER_FRAME: usize = 256;

/// Main event loop: render → poll → handle → repeat.
///
/// The screen is drawn only when something may have changed: after input,
/// and while a search, an export or a timed status message is pending. An
/// idle TUI used to redraw ten times a second for nothing. Input that piled
/// up (a held key, a paste) is handled in one go before the next draw
/// rather than drawing once per key.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    terminate: &AtomicBool,
) -> anyhow::Result<()> {
    let tick_rate = Duration::from_millis(100);
    let mut needs_draw = true;

    loop {
        // Render
        if needs_draw {
            terminal.draw(|frame| {
                ui::render(frame, &mut app);
            })?;
            needs_draw = false;
        }

        // Poll for events
        if ct_poll(tick_rate)? {
            // Anything can follow input, a resize included.
            needs_draw = true;
            let mut handled = 0;
            loop {
                if let Event::Key(key) = ct_read()? {
                    // On Windows (and some terminals with kitty keyboard protocol),
                    // crossterm emits both Press and Release events. Without this filter
                    // every keystroke and pasted character would register twice.
                    if key.kind == KeyEventKind::Press {
                        event::handle_key_event(&mut app, key)?;
                    }
                }
                handled += 1;
                if app.should_quit
                    || app.pending_html_view.is_some()
                    || handled >= MAX_EVENTS_PER_FRAME
                    || !ct_poll(Duration::ZERO)?
                {
                    break;
                }
            }
        }

        // Background work shows progress and results on its own.
        if app.has_pending_work() {
            needs_draw = true;
        }

        // Collect results from any in-flight background search or export.
        app.poll_search();
        app.poll_export();

        // Run a debounced incremental search if the query settled.
        app.poll_incremental_search();

        // Periodic housekeeping
        app.tick();

        // External HTML viewer requested? Suspend TUI, run, restore.
        if let Some(path) = app.pending_html_view.take() {
            run_external_html_viewer(terminal, &path, &mut app)?;
            needs_draw = true;
        }

        if terminate.load(Ordering::Relaxed) {
            app.should_quit = true;
        }

        if app.should_quit {
            // Tell any worker still running to stop.
            app.cancel_search();
            app.cancel_export();
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
    // Not fatal: `clear` asks the terminal where its cursor is, and one that
    // does not answer in time used to end the whole session here. The next
    // draw repaints the screen either way.
    if let Err(e) = terminal.hide_cursor().and_then(|()| terminal.clear()) {
        tracing::warn!(error = %e, "Could not clear the screen after the HTML viewer");
    }

    // Best-effort temp file cleanup.
    let _ = std::fs::remove_file(path);

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => app.set_sticky_status(&format!(
            "{}: {cmd} {} {s}",
            i18n::tui_html_viewer_failed(),
            i18n::tui_exited_with()
        )),
        Err(e) => app.set_sticky_status(&format!(
            "{}: {cmd}: {e} — {}",
            i18n::tui_html_viewer_failed(),
            i18n::tui_html_viewer_hint()
        )),
    }
    Ok(())
}
