//! Keyboard and input event handling.

// The match-then-if pattern (`KeyCode::Char('j') => { if cond { ... } }`)
// reads better here than match guards: each key has one arm, conditions
// stay next to the action they protect, and the wildcard `_ => {}` handles
// the "key matched but condition not met" case naturally.
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::i18n;

use super::app::{
    App, ExportFormat, LayoutMode, PanelFocus, SearchFilterField, SortColumn, SIZE_OPTIONS,
};

/// Process a key event and update the application state.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // ── Ctrl+C quits from anywhere ────────────────────────
    // Checked before any mode that captures keys: raw mode swallows SIGINT,
    // so this is the only emergency exit, and a search prompt used to type a
    // literal "c" instead (a keyboard trap).
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    // A sticky status message (an error, where an export went) has now
    // been seen; this key dismisses it and still does its usual job.
    app.acknowledge_status();

    // ── Help popup (captures all keys) ────────────────────
    // Ahead of the search bar: F1 opens the help over a half-typed query,
    // and Esc must close the help, not abandon the search under it.
    if app.show_help {
        let page = 10;
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::F(1) => {
                app.show_help = false
            }
            KeyCode::Down | KeyCode::Char('j') => app.help_scroll += 1,
            KeyCode::Up | KeyCode::Char('k') => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => app.help_scroll += page,
            KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(page),
            KeyCode::Home | KeyCode::Char('g') => app.help_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => app.help_scroll = app.help_max_scroll,
            _ => {}
        }
        // The render clamps to the real maximum; keep it bounded meanwhile.
        app.help_scroll = app.help_scroll.min(app.help_max_scroll.max(1));
        return Ok(());
    }

    // ── Search bar input mode (captures all keys) ─────────
    if app.search_active {
        return handle_search_input(app, key);
    }

    // ── In-body search prompt (captures all keys) ─────────
    if app.body_search_active {
        return handle_body_search_input(app, key);
    }

    // ── Popup handling (captures all keys) ────────────────
    if app.show_attachments {
        return handle_attachment_popup(app, key);
    }

    if app.show_export {
        return handle_export_popup(app, key);
    }

    if app.show_search_filter {
        return handle_search_filter_popup(app, key);
    }

    // ── Cancel an in-flight background search or export with Esc ────
    if app.search_in_progress() && key.code == KeyCode::Esc {
        app.cancel_search();
        return Ok(());
    }
    if app.export_in_progress() && key.code == KeyCode::Esc {
        app.cancel_export();
        return Ok(());
    }

    // ── Always-available shortcuts ────────────────────────
    match (key.modifiers, key.code) {
        // Help toggle from any panel
        (_, KeyCode::Char('?')) => {
            app.show_help = true;
            app.help_scroll = 0;
            return Ok(());
        }
        // F1: the help, scrolled to the search syntax
        (_, KeyCode::F(1)) => {
            app.open_search_syntax_help();
            return Ok(());
        }
        // Tab: cycle focus forward
        (_, KeyCode::Tab) => {
            app.focus = next_focus(app, true);
            return Ok(());
        }
        // Shift+Tab: cycle focus backward
        (_, KeyCode::BackTab) => {
            app.focus = next_focus(app, false);
            return Ok(());
        }
        // Global message search — from any panel except the sidebar and the
        // message view. With the message view focused, '/' searches within the
        // open body instead (handled in `handle_mail_view_keys`).
        (_, KeyCode::Char('/'))
            if app.focus != PanelFocus::Sidebar && app.focus != PanelFocus::MailView =>
        {
            app.open_search_bar();
            return Ok(());
        }
        // Layout shortcuts
        (_, KeyCode::Char('1')) => {
            app.layout = LayoutMode::ListOnly;
            return Ok(());
        }
        (_, KeyCode::Char('2')) => {
            app.layout = LayoutMode::HorizontalSplit;
            return Ok(());
        }
        (_, KeyCode::Char('3')) => {
            app.layout = LayoutMode::VerticalSplit;
            return Ok(());
        }
        // l: toggle/focus sidebar (from any panel). Uppercase L kept as a
        // hidden alias for backward compatibility.
        (_, KeyCode::Char('l') | KeyCode::Char('L')) => {
            handle_sidebar_toggle(app);
            return Ok(());
        }
        _ => {}
    }

    // ── Panel-specific shortcuts ──────────────────────────
    match app.focus {
        PanelFocus::Sidebar => handle_sidebar_keys(app, key),
        PanelFocus::MailList => handle_mail_list_keys(app, key),
        PanelFocus::MailView => handle_mail_view_keys(app, key),
        PanelFocus::SearchBar => Ok(()),
    }
}

/// Cycle focus to the next (or previous) panel.
fn next_focus(app: &App, forward: bool) -> PanelFocus {
    let has_sidebar = app.show_sidebar && !app.all_labels.is_empty();

    if forward {
        match app.focus {
            PanelFocus::Sidebar => PanelFocus::MailList,
            PanelFocus::MailList => PanelFocus::MailView,
            PanelFocus::MailView => {
                if has_sidebar {
                    PanelFocus::Sidebar
                } else {
                    PanelFocus::MailList
                }
            }
            PanelFocus::SearchBar => PanelFocus::MailList,
        }
    } else {
        match app.focus {
            PanelFocus::Sidebar => PanelFocus::MailView,
            PanelFocus::MailList => {
                if has_sidebar {
                    PanelFocus::Sidebar
                } else {
                    PanelFocus::MailView
                }
            }
            PanelFocus::MailView => PanelFocus::MailList,
            PanelFocus::SearchBar => PanelFocus::MailList,
        }
    }
}

/// Handle the L key: toggle sidebar visibility and focus.
fn handle_sidebar_toggle(app: &mut App) {
    if app.all_labels.is_empty() {
        app.set_status(i18n::tui_no_labels());
        return;
    }

    if !app.show_sidebar {
        // Sidebar hidden → show and focus it
        app.show_sidebar = true;
        app.focus = PanelFocus::Sidebar;
    } else if app.focus == PanelFocus::Sidebar {
        // Sidebar focused → unfocus back to MailList
        app.focus = PanelFocus::MailList;
    } else {
        // Sidebar visible but not focused → focus it
        app.focus = PanelFocus::Sidebar;
    }
}

/// Key handling when the mail list panel has focus.
fn handle_mail_list_keys(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        // ── Scroll the selected message body without leaving the list ──
        // Shift+Down / Shift+Up scroll the preview pane of the currently
        // selected message while keeping focus on the list (issue #8).
        KeyCode::Down if shift => {
            app.message_scroll_offset += 1;
        }
        KeyCode::Up if shift => {
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown if shift => {
            let page = app.message_view_height.max(1);
            app.message_scroll_offset += page;
        }
        KeyCode::PageUp if shift => {
            let page = app.message_view_height.max(1);
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(page);
        }

        // ── Navigation ───────────────────────────────────────
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected + 1 < app.visible_count() {
                app.select_message(app.selected + 1);
                app.ensure_selected_visible();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected > 0 {
                app.select_message(app.selected - 1);
                app.ensure_selected_visible();
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.select_message(0);
            app.ensure_selected_visible();
        }
        KeyCode::Char('G') | KeyCode::End => {
            let last = app.visible_count().saturating_sub(1);
            app.select_message(last);
            app.ensure_selected_visible();
        }
        KeyCode::PageDown => {
            let page = app.list_viewport_height.max(1);
            let new_idx = (app.selected + page).min(app.visible_count().saturating_sub(1));
            app.select_message(new_idx);
            app.ensure_selected_visible();
        }
        KeyCode::PageUp => {
            let page = app.list_viewport_height.max(1);
            let new_idx = app.selected.saturating_sub(page);
            app.select_message(new_idx);
            app.ensure_selected_visible();
        }

        // ── Actions ──────────────────────────────────────────
        KeyCode::Enter => {
            // In ListOnly the view renders fullscreen on focus change,
            // so keep the layout and just move focus.
            app.focus = PanelFocus::MailView;
        }
        KeyCode::Char(' ') => app.toggle_mark(),
        KeyCode::Char('*') => {
            // Toggle based on whether the currently visible rows are all marked
            // — `marked` is a global set of offsets, so its length is not "how
            // many visible rows are marked" once a filter is active.
            let all_visible_marked = !app.visible_indices.is_empty()
                && app
                    .visible_indices
                    .iter()
                    .all(|&i| app.marked.contains(&app.entries[i].offset));
            if all_visible_marked {
                for &idx in &app.visible_indices {
                    app.marked.remove(&app.entries[idx].offset);
                }
            } else {
                for &idx in &app.visible_indices {
                    app.marked.insert(app.entries[idx].offset);
                }
            }
        }

        // ── Sorting ──────────────────────────────────────────
        KeyCode::Char('s') => {
            let next = match app.sort_column {
                SortColumn::Date => SortColumn::From,
                SortColumn::From => SortColumn::Subject,
                SortColumn::Subject => SortColumn::Size,
                SortColumn::Size => SortColumn::Date,
            };
            app.sort_by(next);
            let col_name = match app.sort_column {
                SortColumn::Date => i18n::tui_col_date(),
                SortColumn::From => i18n::tui_col_from(),
                SortColumn::Subject => i18n::tui_col_subject(),
                SortColumn::Size => i18n::tui_col_size(),
            };
            let dir = if app.sort_ascending {
                i18n::tui_sort_asc()
            } else {
                i18n::tui_sort_desc()
            };
            app.set_status(&format!("{} {col_name} ({dir})", i18n::tui_sorted_by()));
        }
        KeyCode::Char('S') => {
            app.sort_ascending = !app.sort_ascending;
            app.apply_sort();
            app.ensure_selected_visible();
        }

        // ── Feature toggles ─────────────────────────────────
        KeyCode::Char('a') => {
            app.attachment_selected = 0;
            app.show_attachments = true;
        }
        KeyCode::Char('e') => {
            app.export_selected = 0;
            app.show_export = true;
        }
        KeyCode::Char('h') => app.show_full_headers = !app.show_full_headers,
        KeyCode::Char('H') => app.request_external_html_view(),
        KeyCode::Char('r') => app.show_raw = !app.show_raw,
        // f opens the search-filter popup. Uppercase F kept as a hidden alias.
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.reset_search_filters();
            app.show_search_filter = true;
        }
        KeyCode::Char('t') => app.toggle_threads(),

        // ── Search navigation ────────────────────────────────
        KeyCode::Char('n') => {
            if !app.search_results.is_empty() {
                app.search_result_index = (app.search_result_index + 1) % app.search_results.len();
                let idx = app.search_results[app.search_result_index];
                if let Some(pos) = app.visible_indices.iter().position(|&i| i == idx) {
                    app.select_message(pos);
                    app.ensure_selected_visible();
                }
            }
        }
        KeyCode::Char('N') => {
            if !app.search_results.is_empty() {
                app.search_result_index = if app.search_result_index == 0 {
                    app.search_results.len() - 1
                } else {
                    app.search_result_index - 1
                };
                let idx = app.search_results[app.search_result_index];
                if let Some(pos) = app.visible_indices.iter().position(|&i| i == idx) {
                    app.select_message(pos);
                    app.ensure_selected_visible();
                }
            }
        }

        // ── Quit ─────────────────────────────────────────────
        KeyCode::Char('q') => {
            app.should_quit = true;
        }

        _ => {}
    }
    Ok(())
}

/// Key handling when the message view panel has focus.
fn handle_mail_view_keys(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.message_scroll_offset += 1;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown => {
            let page = app.message_view_height.max(1);
            app.message_scroll_offset += page;
        }
        KeyCode::PageUp => {
            let page = app.message_view_height.max(1);
            app.message_scroll_offset = app.message_scroll_offset.saturating_sub(page);
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.message_scroll_offset = 0;
        }
        // Open the in-body search prompt (less/vim style).
        KeyCode::Char('/') => app.body_search_open(),
        // Navigate confirmed matches once the prompt is closed.
        KeyCode::Char('n') => app.body_search_next(),
        KeyCode::Char('N') => app.body_search_prev(),
        KeyCode::Esc => {
            // Esc escalates: first dismiss any lingering matches, then leave.
            if !app.body_search_matches.is_empty() {
                app.body_search_clear();
            } else {
                app.focus = PanelFocus::MailList;
            }
        }
        KeyCode::Char('h') => app.show_full_headers = !app.show_full_headers,
        KeyCode::Char('H') => app.request_external_html_view(),
        KeyCode::Char('r') => app.show_raw = !app.show_raw,
        KeyCode::Char('a') => {
            app.attachment_selected = 0;
            app.show_attachments = true;
        }
        KeyCode::Char('e') => {
            app.export_selected = 0;
            app.show_export = true;
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        _ => {}
    }
    Ok(())
}

/// Key handling while the in-body search prompt is open and capturing input.
///
/// Typing refines the query with live highlighting; Enter confirms (closing the
/// prompt but keeping matches navigable with n/N); Esc cancels and clears.
fn handle_body_search_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.body_search_clear();
        }
        KeyCode::Enter => {
            // Confirm: keep matches highlighted, close the prompt so n/N navigate.
            app.body_search_active = false;
        }
        KeyCode::Backspace => {
            app.body_search_query.pop();
            app.recompute_body_matches();
        }
        _ if is_clear_line(&key) => {
            app.body_search_query.clear();
            app.recompute_body_matches();
        }
        KeyCode::Char(_) => {
            if let Some(c) = typed_char(&key) {
                app.body_search_query.push(c);
                app.recompute_body_matches();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Key handling when the sidebar (labels) panel has focus.
fn handle_sidebar_keys(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let total = 1 + app.all_labels.len(); // "All Messages" + labels
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.sidebar_selected + 1 < total {
                app.sidebar_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.sidebar_selected > 0 {
                app.sidebar_selected -= 1;
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.sidebar_selected = 0;
        }
        KeyCode::Char('G') | KeyCode::End => {
            app.sidebar_selected = total.saturating_sub(1);
        }
        KeyCode::Enter => {
            // Apply label filter and move focus to mail list
            if app.sidebar_selected == 0 {
                app.apply_label_filter(None);
            } else if let Some(label) = app.all_labels.get(app.sidebar_selected - 1) {
                app.apply_label_filter(Some(label.clone()));
            }
            app.focus = PanelFocus::MailList;
        }
        KeyCode::Esc => {
            // Leave sidebar, go back to mail list
            app.focus = PanelFocus::MailList;
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        _ => {}
    }
    Ok(())
}

/// Key handling when the attachment popup is open.
fn handle_attachment_popup(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let count = app
        .current_body
        .as_ref()
        .map(|b| b.attachments.len())
        .unwrap_or(0);

    match key.code {
        KeyCode::Esc | KeyCode::Char('a') => app.show_attachments = false,
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 && app.attachment_selected + 1 < count {
                app.attachment_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.attachment_selected > 0 {
                app.attachment_selected -= 1;
            }
        }
        KeyCode::Enter => {
            // Save selected attachment to ~/Downloads (or Desktop as fallback)
            if count > 0 {
                let output_dir = default_download_dir();
                match save_single_attachment(app, app.attachment_selected, &output_dir) {
                    Ok(path) => {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                        app.set_sticky_status(&format!(
                            "{}: {name} -> {}",
                            i18n::tui_saved(),
                            output_dir.display()
                        ));
                        app.show_attachments = false;
                    }
                    Err(e) => {
                        app.set_sticky_status(&format!("{}: {e}", i18n::tui_error_saving()));
                    }
                }
            }
        }
        KeyCode::Char('A') => {
            // Save all attachments
            if count > 0 {
                let output_dir = default_download_dir();
                match save_all_attachments(app, &output_dir) {
                    Ok(paths) => {
                        app.set_sticky_status(&format!(
                            "{} {} {} -> {}",
                            i18n::tui_saved(),
                            paths.len(),
                            i18n::tui_attachments_count(),
                            output_dir.display()
                        ));
                        app.show_attachments = false;
                    }
                    Err(e) => {
                        app.set_sticky_status(&format!("{}: {e}", i18n::tui_error_saving_all()));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Key handling when the export popup is open.
fn handle_export_popup(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let option_count = crate::tui::widgets::export_popup::EXPORT_OPTION_COUNT;
    match key.code {
        KeyCode::Esc => app.show_export = false,
        KeyCode::Char('j') | KeyCode::Down => {
            if app.export_selected + 1 < option_count {
                app.export_selected += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.export_selected > 0 {
                app.export_selected -= 1;
            }
        }
        KeyCode::Enter => {
            let output_dir = default_download_dir();
            match app.export_selected {
                0 => {
                    // EML export
                    match export_current_eml(app, &output_dir) {
                        Ok(msg) => {
                            app.set_sticky_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => {
                            app.set_sticky_status(&format!("{}: {e}", i18n::tui_export_error()))
                        }
                    }
                }
                1 => {
                    // HTML export
                    match export_current_html(app, &output_dir) {
                        Ok(msg) => {
                            app.set_sticky_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => {
                            app.set_sticky_status(&format!("{}: {e}", i18n::tui_export_error()))
                        }
                    }
                }
                2 => {
                    // TXT export
                    match export_current_txt(app, &output_dir) {
                        Ok(msg) => {
                            app.set_sticky_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => {
                            app.set_sticky_status(&format!("{}: {e}", i18n::tui_export_error()))
                        }
                    }
                }
                3 => {
                    // CSV export
                    match export_current_csv(app, &output_dir) {
                        Ok(msg) => {
                            app.set_sticky_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => {
                            app.set_sticky_status(&format!("{}: {e}", i18n::tui_export_error()))
                        }
                    }
                }
                4 => {
                    // Attachments
                    let att_count = app
                        .current_body
                        .as_ref()
                        .map(|b| b.attachments.len())
                        .unwrap_or(0);
                    if att_count == 0 {
                        app.set_sticky_status(i18n::tui_no_attachments_msg());
                    } else {
                        match save_all_attachments(app, &output_dir) {
                            Ok(paths) => {
                                app.set_sticky_status(&format!(
                                    "{} {} {} -> {}",
                                    i18n::tui_saved(),
                                    paths.len(),
                                    i18n::tui_attachments_count(),
                                    output_dir.display()
                                ));
                                app.show_export = false;
                            }
                            Err(e) => app.set_sticky_status(&format!("{}: {e}", i18n::tui_error())),
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Return the default download directory (~/Downloads or ~/Desktop as fallback).
fn default_download_dir() -> PathBuf {
    if let Some(dir) = dirs::download_dir() {
        return dir;
    }
    if let Some(dir) = dirs::desktop_dir() {
        return dir;
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Hand the marked messages to a background export and return the status
/// line to show while it runs.
fn export_marked_in_background(
    app: &mut App,
    format: ExportFormat,
    output_dir: &std::path::Path,
) -> String {
    app.start_export_marked(format, output_dir.to_path_buf());
    // `start_export_marked` set the status (progress, or "already running").
    app.status_message
        .as_ref()
        .map(|(msg, _)| msg.clone())
        .unwrap_or_default()
}

/// Export the current (or marked) message(s) as EML.
fn export_current_eml(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    if !app.marked.is_empty() {
        Ok(export_marked_in_background(
            app,
            ExportFormat::Eml,
            output_dir,
        ))
    } else if let Some(entry) = app.current_entry().cloned() {
        let path = crate::export::eml::export_eml(&mut app.store, &entry, output_dir)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("message.eml");
        Ok(format!(
            "{}: {name} -> {}",
            i18n::tui_exported(),
            output_dir.display()
        ))
    } else {
        Ok(i18n::tui_no_message().to_string())
    }
}

/// Sanitized HTML export with the message's inline (`cid:`) images embedded.
fn export_html_with_images(
    app: &mut App,
    entry: &crate::model::mail::MailEntry,
    body: &crate::model::mail::MailBody,
    output_dir: &std::path::Path,
) -> anyhow::Result<PathBuf> {
    use crate::export::html::{export_html_with, HtmlOptions, InlineImages};
    let opts = HtmlOptions {
        inline_images: InlineImages::load(&mut app.store, entry, body)?,
        ..HtmlOptions::default()
    };
    export_html_with(entry, body, output_dir, &opts)
}

/// Export the current message — or all marked messages — as standalone HTML.
fn export_current_html(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    if !app.marked.is_empty() {
        Ok(export_marked_in_background(
            app,
            ExportFormat::Html,
            output_dir,
        ))
    } else if let (Some(entry), Some(body)) =
        (app.current_entry().cloned(), app.current_body.clone())
    {
        let path = export_html_with_images(app, &entry, &body, output_dir)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("message.html");
        Ok(format!(
            "{}: {name} -> {}",
            i18n::tui_exported(),
            output_dir.display()
        ))
    } else {
        Ok(i18n::tui_no_message().to_string())
    }
}

/// Export the current message — or all marked messages — as TXT.
fn export_current_txt(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    if !app.marked.is_empty() {
        Ok(export_marked_in_background(
            app,
            ExportFormat::Txt,
            output_dir,
        ))
    } else if let (Some(entry), Some(body)) =
        (app.current_entry().cloned(), app.current_body.clone())
    {
        let path = crate::export::text::export_text(&entry, &body, output_dir)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("message.txt");
        Ok(format!(
            "{}: {name} -> {}",
            i18n::tui_exported(),
            output_dir.display()
        ))
    } else {
        Ok(i18n::tui_no_message().to_string())
    }
}

/// Export the current (or marked) message(s) metadata as CSV.
fn export_current_csv(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    let entries: Vec<&crate::model::mail::MailEntry> = if !app.marked.is_empty() {
        app.entries
            .iter()
            .filter(|e| app.marked.contains(&e.offset))
            .collect()
    } else if let Some(entry) = app.current_entry() {
        vec![entry]
    } else {
        return Ok(i18n::tui_no_message().to_string());
    };

    let count = entries.len();
    // Reserve a free name (`mboxshell_export_1.csv`, …) so a second export
    // never overwrites the first; export_csv then renames its temp over the
    // empty placeholder.
    let (csv_path, _) = crate::fsutil::create_unique(&output_dir.join("mboxshell_export.csv"))?;
    if let Err(e) = crate::export::csv::export_csv(&entries, &csv_path, None) {
        let _ = std::fs::remove_file(&csv_path);
        return Err(e);
    }
    Ok(format!(
        "{} {count} {} -> {}",
        i18n::tui_exported(),
        i18n::tui_exported_csv(),
        csv_path.display()
    ))
}

/// Save a single attachment from the current message by index.
fn save_single_attachment(
    app: &mut App,
    att_index: usize,
    output_dir: &PathBuf,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(output_dir)?;

    let entry = app
        .current_entry()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}", i18n::tui_no_message()))?;

    let attachment = app
        .current_body
        .as_ref()
        .and_then(|b| b.attachments.get(att_index))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}", i18n::tui_attachment_not_found()))?;

    crate::export::attachment::export_attachment(&mut app.store, &entry, &attachment, output_dir)
}

/// Save all attachments from the current message.
fn save_all_attachments(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;

    let entry = app
        .current_entry()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}", i18n::tui_no_message()))?;

    crate::export::attachment::export_all_attachments(&mut app.store, &entry, output_dir)
}

/// Key handling when the search bar is active.
fn handle_search_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        // `?` is a character a query may contain, so the syntax lives on F1.
        KeyCode::F(1) => app.open_search_syntax_help(),
        KeyCode::Esc => {
            app.search_active = false;
            app.search_history_index = None;
            app.focus = PanelFocus::MailList;
            // Abandon any debounced incremental recompute for the closed query,
            // and any background scan still running for an earlier one.
            app.cancel_pending_incremental_search();
            app.cancel_search();
            app.search_bar_scope = None;
            // Leaving the search resets the scope, so drop within-results mode.
            app.filter_within_results = false;
            // Reset to show all messages (respecting active label filter)
            if let Some(label) = app.active_label_filter.clone() {
                app.apply_label_filter(Some(label));
            } else {
                app.visible_indices = (0..app.entries.len()).collect();
                app.search_results.clear();
                app.apply_sort();
                if !app.visible_indices.is_empty() {
                    app.select_message(0);
                }
            }
        }
        KeyCode::Enter => {
            app.push_search_history(&app.search_query.clone());
            app.search_history_index = None;
            app.execute_search();
            app.search_active = false;
            app.focus = PanelFocus::MailList;
        }
        KeyCode::Up => {
            // Navigate backward through history
            if !app.search_history.is_empty() {
                match app.search_history_index {
                    None => {
                        // Save current query as draft and load first history entry
                        app.search_draft = app.search_query.clone();
                        app.search_history_index = Some(0);
                        app.search_query = app.search_history[0].clone();
                    }
                    Some(idx) => {
                        let next = idx + 1;
                        if next < app.search_history.len() {
                            app.search_history_index = Some(next);
                            app.search_query = app.search_history[next].clone();
                        }
                    }
                }
                app.mark_search_dirty();
            }
        }
        KeyCode::Down => {
            // Navigate forward through history (toward draft)
            if let Some(idx) = app.search_history_index {
                if idx == 0 {
                    // Restore the original draft
                    app.search_history_index = None;
                    app.search_query = app.search_draft.clone();
                } else {
                    let prev = idx - 1;
                    app.search_history_index = Some(prev);
                    app.search_query = app.search_history[prev].clone();
                }
                app.mark_search_dirty();
            }
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.search_history_index = None;
            app.mark_search_dirty();
        }
        _ if is_clear_line(&key) => {
            app.search_query.clear();
            app.search_history_index = None;
            app.mark_search_dirty();
        }
        KeyCode::Char(_) => {
            if let Some(c) = typed_char(&key) {
                app.search_query.push(c);
                app.search_history_index = None;
                app.mark_search_dirty();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Key handling when the search filter popup is open.
fn handle_search_filter_popup(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let has_labels = !app.all_labels.is_empty();
    let focus = app.search_filter_focus;

    match key.code {
        KeyCode::Esc => {
            app.show_search_filter = false;
        }
        // Field navigation: Tab/Down move forward, Shift-Tab/Up move back.
        // Arrow keys mirror Tab so vertical movement works across every field,
        // including the Size/Label selectors (whose value is changed with
        // Left/Right instead — see below).
        KeyCode::Tab | KeyCode::Down => {
            app.search_filter_focus = focus.next(has_labels);
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.search_filter_focus = focus.prev(has_labels);
        }
        // Page Up/Down (and Home/End) jump to the first/last field.
        KeyCode::PageUp | KeyCode::Home => {
            app.search_filter_focus = SearchFilterField::first();
        }
        KeyCode::PageDown | KeyCode::End => {
            app.search_filter_focus = SearchFilterField::last();
        }
        KeyCode::Enter => {
            // Build the query from the filters, then run it through the shared
            // search path. `execute_search` preserves the active scope —
            // "within previous results" (the currently visible messages) or an
            // active label filter — intersecting it with the results when they
            // arrive (the search may run on a background thread).
            let query = app.build_query_from_filters();
            app.search_query = query.clone();
            app.push_search_history(&query);
            app.show_search_filter = false;
            app.execute_search();
        }
        KeyCode::Char(' ') if focus == SearchFilterField::HasAttachment => {
            app.filter_has_attachment = !app.filter_has_attachment;
        }
        KeyCode::Char(' ') if focus == SearchFilterField::WithinResults => {
            app.filter_within_results = !app.filter_within_results;
        }
        // Selector value change: Right/l/j advance, Left/h/k go back. Up/Down are
        // reserved for field navigation, so horizontal keys drive the selectors.
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('j')
            if focus == SearchFilterField::Size =>
        {
            let max = SIZE_OPTIONS.len().saturating_sub(1);
            if app.filter_size_selected < max {
                app.filter_size_selected += 1;
            }
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('k')
            if focus == SearchFilterField::Size =>
        {
            if app.filter_size_selected > 0 {
                app.filter_size_selected -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('j')
            if focus == SearchFilterField::Label =>
        {
            let max = app.all_labels.len(); // 0=Any, so max index = labels.len()
            if app.filter_label_selected < max {
                app.filter_label_selected += 1;
            }
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('k')
            if focus == SearchFilterField::Label =>
        {
            if app.filter_label_selected > 0 {
                app.filter_label_selected -= 1;
            }
        }
        KeyCode::Backspace if focus.is_text_input() => match focus {
            SearchFilterField::Text => {
                app.filter_text.pop();
            }
            SearchFilterField::From => {
                app.filter_from.pop();
            }
            SearchFilterField::To => {
                app.filter_to.pop();
            }
            SearchFilterField::Subject => {
                app.filter_subject.pop();
            }
            SearchFilterField::DateFrom => {
                app.filter_date_from.pop();
            }
            SearchFilterField::DateTo => {
                app.filter_date_to.pop();
            }
            _ => {}
        },
        _ if focus.is_text_input() && is_clear_line(&key) => {
            if let Some(field) = filter_field_mut(app, focus) {
                field.clear();
            }
        }
        KeyCode::Char(_) if focus.is_text_input() => {
            if let (Some(c), Some(field)) = (typed_char(&key), filter_field_mut(app, focus)) {
                field.push(c);
            }
        }
        _ => {}
    }
    Ok(())
}

/// The text buffer behind a text-input field of the filter popup.
fn filter_field_mut(app: &mut App, field: SearchFilterField) -> Option<&mut String> {
    match field {
        SearchFilterField::Text => Some(&mut app.filter_text),
        SearchFilterField::From => Some(&mut app.filter_from),
        SearchFilterField::To => Some(&mut app.filter_to),
        SearchFilterField::Subject => Some(&mut app.filter_subject),
        SearchFilterField::DateFrom => Some(&mut app.filter_date_from),
        SearchFilterField::DateTo => Some(&mut app.filter_date_to),
        _ => None,
    }
}

/// The character a key types into a text field, if it types one at all.
///
/// Ctrl or Alt chords are commands, not text: they used to be inserted as
/// their bare letter (Ctrl+W typed "w"). Shift is part of typing, and
/// Ctrl+Alt together is how Windows reports AltGr, which types real
/// characters (`@`, `#`, `€` on many layouts).
fn typed_char(key: &KeyEvent) -> Option<char> {
    let KeyCode::Char(c) = key.code else {
        return None;
    };
    let chord = key.modifiers.difference(KeyModifiers::SHIFT);
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    (chord.is_empty() || chord.contains(altgr)).then_some(c)
}

/// Ctrl+U: clear the whole input, as in a shell prompt.
fn is_clear_line(key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('u')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn f1_in_the_search_bar_shows_the_syntax_and_esc_keeps_the_query() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_active = true;
        app.search_query = "from:al".to_string();

        handle_key_event(&mut app, key(KeyCode::F(1), KeyModifiers::NONE)).expect("F1");
        assert!(app.show_help && app.help_jump_to_syntax);
        assert_eq!(
            app.search_query, "from:al",
            "F1 is not typed into the query"
        );

        // Keys now drive the help, not the query under it.
        handle_key_event(&mut app, key(KeyCode::Char('j'), KeyModifiers::NONE)).expect("j");
        assert_eq!(app.search_query, "from:al");

        handle_key_event(&mut app, key(KeyCode::Esc, KeyModifiers::NONE)).expect("Esc");
        assert!(!app.show_help, "Esc closes the help");
        assert!(app.search_active, "...and leaves the search bar open");
        assert_eq!(app.search_query, "from:al");
    }

    #[test]
    fn ctrl_c_quits_even_from_a_text_prompt_or_popup() {
        for setup in [
            (|a: &mut App| a.search_active = true) as fn(&mut App),
            |a| a.body_search_active = true,
            |a| a.show_help = true,
            |a| a.show_search_filter = true,
        ] {
            let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
            setup(&mut app);
            handle_key_event(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap();
            assert!(app.should_quit);
            assert!(app.search_query.is_empty(), "no stray 'c' typed");
        }
    }

    #[test]
    fn text_prompts_ignore_chords_but_accept_altgr_and_ctrl_u() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_active = true;
        for (code, mods) in [
            (KeyCode::Char('a'), KeyModifiers::NONE),
            (KeyCode::Char('B'), KeyModifiers::SHIFT),
            (KeyCode::Char('w'), KeyModifiers::CONTROL),
            (KeyCode::Char('x'), KeyModifiers::ALT),
            // AltGr as Windows reports it.
            (
                KeyCode::Char('@'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        ] {
            handle_key_event(&mut app, key(code, mods)).unwrap();
        }
        assert_eq!(app.search_query, "aB@");

        handle_key_event(&mut app, key(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn sticky_status_outlives_the_timeout_until_a_key_is_pressed() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.set_sticky_status("boom");
        if let Some((_, when)) = app.status_message.as_mut() {
            *when -= std::time::Duration::from_secs(60);
        }
        app.tick();
        assert!(
            app.status_message.is_some(),
            "a sticky message must not time out"
        );

        handle_key_event(&mut app, key(KeyCode::Char('j'), KeyModifiers::NONE)).unwrap();
        assert!(app.status_message.is_none(), "any key dismisses it");
    }

    /// Regression for #20: exporting with several messages marked must write
    /// one file per marked message for HTML and TXT (not just the current one).
    #[test]
    fn export_html_and_txt_cover_all_marked_messages() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        assert!(
            app.entries.len() >= 2,
            "fixture needs at least two messages"
        );
        app.marked.insert(app.entries[0].offset);
        app.marked.insert(app.entries[1].offset);

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().to_path_buf();

        // Marked messages are exported on a background thread: wait for it
        // the way the main loop does, by polling.
        fn wait_for_export(app: &mut App) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while app.export_in_progress() {
                assert!(std::time::Instant::now() < deadline, "export never ended");
                std::thread::sleep(std::time::Duration::from_millis(5));
                app.poll_export();
            }
        }

        export_current_html(&mut app, &out).unwrap();
        assert!(app.export_in_progress(), "the key handler must not block");
        // A second export while one runs is refused, not queued on top.
        export_current_txt(&mut app, &out).unwrap();
        wait_for_export(&mut app);
        let status = app
            .status_message
            .clone()
            .map(|(m, _)| m)
            .unwrap_or_default();
        assert!(
            status.contains("2 "),
            "final status reports the count: {status}"
        );
        assert!(app.status_sticky);

        export_current_txt(&mut app, &out).unwrap();
        wait_for_export(&mut app);

        let count_ext = |ext: &str| {
            std::fs::read_dir(&out)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == ext).unwrap_or(false))
                .count()
        };
        assert_eq!(count_ext("html"), 2, "one HTML file per marked message");
        assert_eq!(count_ext("txt"), 2, "one TXT file per marked message");
    }

    #[test]
    fn export_of_marked_messages_can_be_cancelled_with_esc() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.marked.insert(app.entries[0].offset);
        let tmp = tempfile::tempdir().unwrap();
        export_current_eml(&mut app, &tmp.path().to_path_buf()).unwrap();
        assert!(app.export_in_progress());
        handle_key_event(&mut app, key(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
        assert!(!app.export_in_progress());
        assert!(app.has_pending_work() || app.status_sticky);
    }

    /// A list scrolled far down and then shortened by a search drew empty:
    /// the scroll offset stayed past the end of the new list.
    #[test]
    fn list_scroll_follows_the_selection_after_a_search() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.list_viewport_height = 2;
        let last = app.visible_count() - 1;
        app.select_message(last);
        assert!(app.list_scroll_offset > 0);

        app.search_query = format!("id:{}", app.entries[0].message_id);
        app.execute_search();
        assert!(app.visible_count() < last + 1);
        assert_eq!(app.selected, 0);
        assert_eq!(
            app.list_scroll_offset, 0,
            "the first result must be on screen"
        );
    }

    /// A background search that was abandoned must not replace the view when
    /// it finally answers.
    #[test]
    fn abandoned_background_search_does_not_overwrite_the_view() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        let all = app.visible_count();

        // Esc in the search bar while a body search is running.
        app.search_query = "body:zzzz-not-there".into();
        app.execute_search();
        assert!(app.search_in_progress());
        app.search_active = true;
        handle_key_event(&mut app, key(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
        assert!(!app.search_in_progress());
        std::thread::sleep(std::time::Duration::from_millis(100));
        app.poll_search();
        assert_eq!(
            app.visible_count(),
            all,
            "the abandoned search changed the list"
        );

        // A label filter applied while a body search is running.
        app.search_query = "body:zzzz-not-there".into();
        app.execute_search();
        app.apply_label_filter(None);
        assert!(!app.search_in_progress());
        std::thread::sleep(std::time::Duration::from_millis(100));
        app.poll_search();
        assert_eq!(app.visible_count(), all);
    }

    /// With "search within previous results" on, typing in the search bar
    /// narrows the previous results, not the whole mailbox.
    #[test]
    fn search_bar_typing_stays_within_previous_results() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        assert!(app.entries.len() >= 3);
        // "Previous results": just the first two messages of the mailbox.
        app.visible_indices = vec![0, 1];
        app.filter_within_results = true;
        app.open_search_bar();

        // A query every message matches.
        app.search_query = "size:>0b".into();
        app.execute_incremental_search();
        let mut shown = app.visible_indices.clone();
        shown.sort_unstable();
        assert_eq!(shown, vec![0, 1], "typing must stay inside the scope");

        // Clearing the query brings the scope back, not the whole mailbox.
        app.search_query.clear();
        app.execute_incremental_search();
        assert_eq!(app.visible_count(), 2);

        // Enter confirms inside the same scope.
        app.search_query = "size:>0b".into();
        app.execute_search();
        assert_eq!(app.visible_count(), 2);
        assert!(
            app.search_bar_scope.is_none(),
            "the scope ends with the bar"
        );
    }

    /// Sorting in threaded view keeps rows and indentation in step.
    #[test]
    fn sorting_in_threaded_view_keeps_the_tree_consistent() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.toggle_threads();
        let before: Vec<(usize, usize)> = app
            .visible_indices
            .iter()
            .copied()
            .zip(app.thread_depths.iter().copied())
            .collect();
        handle_key_event(&mut app, key(KeyCode::Char('s'), KeyModifiers::NONE)).unwrap();
        handle_key_event(&mut app, key(KeyCode::Char('S'), KeyModifiers::NONE)).unwrap();
        let after: Vec<(usize, usize)> = app
            .visible_indices
            .iter()
            .copied()
            .zip(app.thread_depths.iter().copied())
            .collect();
        assert_eq!(before, after);
        assert_eq!(app.visible_indices.len(), app.thread_depths.len());
    }

    /// Regression for the mark-all (`*`) toggle: it must act on the visible set,
    /// not on `marked.len()` (a global count), and must not clear marks that are
    /// outside the current filtered view.
    #[test]
    fn mark_all_star_toggles_only_the_visible_set() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        assert!(app.entries.len() >= 3, "fixture needs 3+ messages");
        let outside = app.entries[0].offset;
        app.marked.insert(outside); // marked, but outside the filtered view below
        app.visible_indices = vec![1, 2];

        let star = KeyEvent::new(KeyCode::Char('*'), KeyModifiers::NONE);
        // No visible row is marked yet → '*' marks the visible set and leaves
        // the out-of-view mark untouched.
        handle_mail_list_keys(&mut app, star).unwrap();
        assert!(app.marked.contains(&app.entries[1].offset));
        assert!(app.marked.contains(&app.entries[2].offset));
        assert!(
            app.marked.contains(&outside),
            "out-of-view mark must survive"
        );

        // All visible now marked → '*' unmarks the visible set, out-of-view mark intact.
        handle_mail_list_keys(&mut app, star).unwrap();
        assert!(!app.marked.contains(&app.entries[1].offset));
        assert!(!app.marked.contains(&app.entries[2].offset));
        assert!(app.marked.contains(&outside));
    }
}
