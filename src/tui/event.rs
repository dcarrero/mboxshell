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

use super::app::{App, LayoutMode, PanelFocus, SearchFilterField, SortColumn, SIZE_OPTIONS};

/// Process a key event and update the application state.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // ── Search bar input mode (captures all keys) ─────────
    if app.search_active {
        return handle_search_input(app, key);
    }

    // ── In-body search prompt (captures all keys) ─────────
    if app.body_search_active {
        return handle_body_search_input(app, key);
    }

    // ── Popup handling (captures all keys) ────────────────
    if app.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => app.show_help = false,
            _ => {}
        }
        return Ok(());
    }

    if app.show_attachments {
        return handle_attachment_popup(app, key);
    }

    if app.show_export {
        return handle_export_popup(app, key);
    }

    if app.show_search_filter {
        return handle_search_filter_popup(app, key);
    }

    // ── Cancel an in-flight background search with Esc ────
    if app.search_in_progress() && key.code == KeyCode::Esc {
        app.cancel_search();
        return Ok(());
    }

    // ── Always-available shortcuts ────────────────────────
    match (key.modifiers, key.code) {
        // Ctrl+C always quits, from any panel
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
            return Ok(());
        }
        // Help toggle from any panel
        (_, KeyCode::Char('?')) => {
            app.show_help = true;
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
            app.search_active = true;
            app.search_query.clear();
            app.focus = PanelFocus::SearchBar;
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
        KeyCode::Char(c) => {
            app.body_search_query.push(c);
            app.recompute_body_matches();
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
                        app.set_status(&format!(
                            "{}: {name} -> {}",
                            i18n::tui_saved(),
                            output_dir.display()
                        ));
                        app.show_attachments = false;
                    }
                    Err(e) => {
                        app.set_status(&format!("{}: {e}", i18n::tui_error_saving()));
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
                        app.set_status(&format!(
                            "{} {} {} -> {}",
                            i18n::tui_saved(),
                            paths.len(),
                            i18n::tui_attachments_count(),
                            output_dir.display()
                        ));
                        app.show_attachments = false;
                    }
                    Err(e) => {
                        app.set_status(&format!("{}: {e}", i18n::tui_error_saving_all()));
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
                            app.set_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => app.set_status(&format!("{}: {e}", i18n::tui_export_error())),
                    }
                }
                1 => {
                    // HTML export
                    match export_current_html(app, &output_dir) {
                        Ok(msg) => {
                            app.set_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => app.set_status(&format!("{}: {e}", i18n::tui_export_error())),
                    }
                }
                2 => {
                    // TXT export
                    match export_current_txt(app, &output_dir) {
                        Ok(msg) => {
                            app.set_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => app.set_status(&format!("{}: {e}", i18n::tui_export_error())),
                    }
                }
                3 => {
                    // CSV export
                    match export_current_csv(app, &output_dir) {
                        Ok(msg) => {
                            app.set_status(&msg);
                            app.show_export = false;
                        }
                        Err(e) => app.set_status(&format!("{}: {e}", i18n::tui_export_error())),
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
                        app.set_status(i18n::tui_no_attachments_msg());
                    } else {
                        match save_all_attachments(app, &output_dir) {
                            Ok(paths) => {
                                app.set_status(&format!(
                                    "{} {} {} -> {}",
                                    i18n::tui_saved(),
                                    paths.len(),
                                    i18n::tui_attachments_count(),
                                    output_dir.display()
                                ));
                                app.show_export = false;
                            }
                            Err(e) => app.set_status(&format!("{}: {e}", i18n::tui_error())),
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

/// Export the current (or marked) message(s) as EML.
fn export_current_eml(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    if !app.marked.is_empty() {
        let entries: Vec<&crate::model::mail::MailEntry> = app
            .entries
            .iter()
            .filter(|e| app.marked.contains(&e.offset))
            .collect();
        let count = entries.len();
        crate::export::eml::export_multiple_eml(&mut app.store, &entries, output_dir, &|_, _| {})?;
        Ok(format!(
            "{} {count} {} -> {}",
            i18n::tui_exported(),
            i18n::tui_exported_messages_eml(),
            output_dir.display()
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

/// Export the current message — or all marked messages — as standalone HTML.
fn export_current_html(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<String> {
    std::fs::create_dir_all(output_dir)?;

    if !app.marked.is_empty() {
        let entries: Vec<crate::model::mail::MailEntry> = app
            .entries
            .iter()
            .filter(|e| app.marked.contains(&e.offset))
            .cloned()
            .collect();
        let count = entries.len();
        for entry in &entries {
            let body = app.store.get_message(entry)?;
            crate::export::html::export_html(entry, &body, output_dir)?;
        }
        Ok(format!(
            "{} {count} {} -> {}",
            i18n::tui_exported(),
            i18n::tui_exported_messages_html(),
            output_dir.display()
        ))
    } else if let (Some(entry), Some(body)) =
        (app.current_entry().cloned(), app.current_body.clone())
    {
        let path = crate::export::html::export_html(&entry, &body, output_dir)?;
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
        let entries: Vec<crate::model::mail::MailEntry> = app
            .entries
            .iter()
            .filter(|e| app.marked.contains(&e.offset))
            .cloned()
            .collect();
        let count = entries.len();
        for entry in &entries {
            let body = app.store.get_message(entry)?;
            crate::export::text::export_text(entry, &body, output_dir)?;
        }
        Ok(format!(
            "{} {count} {} -> {}",
            i18n::tui_exported(),
            i18n::tui_exported_messages_txt(),
            output_dir.display()
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
    let csv_path = output_dir.join("mboxshell_export.csv");
    crate::export::csv::export_csv(&entries, &csv_path, None)?;
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
        .ok_or_else(|| anyhow::anyhow!("No message selected"))?;

    let attachment = app
        .current_body
        .as_ref()
        .and_then(|b| b.attachments.get(att_index))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Attachment not found"))?;

    crate::export::attachment::export_attachment(&mut app.store, &entry, &attachment, output_dir)
}

/// Save all attachments from the current message.
fn save_all_attachments(app: &mut App, output_dir: &PathBuf) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;

    let entry = app
        .current_entry()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No message selected"))?;

    crate::export::attachment::export_all_attachments(&mut app.store, &entry, output_dir)
}

/// Key handling when the search bar is active.
fn handle_search_input(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.search_active = false;
            app.search_history_index = None;
            app.focus = PanelFocus::MailList;
            // Abandon any debounced incremental recompute for the closed query.
            app.cancel_pending_incremental_search();
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
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.search_history_index = None;
            app.mark_search_dirty();
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
        KeyCode::Char(c) if focus.is_text_input() => match focus {
            SearchFilterField::Text => app.filter_text.push(c),
            SearchFilterField::From => app.filter_from.push(c),
            SearchFilterField::To => app.filter_to.push(c),
            SearchFilterField::Subject => app.filter_subject.push(c),
            SearchFilterField::DateFrom => app.filter_date_from.push(c),
            SearchFilterField::DateTo => app.filter_date_to.push(c),
            _ => {}
        },
        _ => {}
    }
    Ok(())
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

        export_current_html(&mut app, &out).unwrap();
        export_current_txt(&mut app, &out).unwrap();

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
