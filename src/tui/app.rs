//! Global application state for the TUI (the "Model" in Elm architecture).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::index::builder;
use crate::model::mail::{MailBody, MailEntry};
use crate::store::reader::MboxStore;
use crate::tui::threading;

/// Which panel currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    Sidebar,
    MailList,
    MailView,
    SearchBar,
}

/// Layout arrangement for list and message panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Only the message list is visible.
    ListOnly,
    /// List on top, message below.
    HorizontalSplit,
    /// List on the left, message on the right.
    VerticalSplit,
}

/// Column used for sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Date,
    From,
    Subject,
    Size,
}

/// Complete TUI state.
pub struct App {
    // ── Data ──────────────────────────────────
    /// Path to the open MBOX file.
    pub mbox_path: PathBuf,
    /// Full index of messages (in memory).
    pub entries: Vec<MailEntry>,
    /// Indices into `entries` for the currently visible (filtered) messages.
    pub visible_indices: Vec<usize>,
    /// Store for random-access message reading.
    pub store: MboxStore,

    // ── Navigation ────────────────────────────
    /// Index within `visible_indices` of the selected message.
    pub selected: usize,
    /// Scroll offset for the list widget.
    pub list_scroll_offset: usize,
    /// Scroll offset for the message view widget.
    pub message_scroll_offset: usize,
    /// Set of offsets for "marked" messages (toggled with Space).
    pub marked: HashSet<u64>,

    // ── UI state ──────────────────────────────
    /// Active panel.
    pub focus: PanelFocus,
    /// Layout mode.
    pub layout: LayoutMode,
    /// Help popup visible?
    pub show_help: bool,
    /// Attachment popup visible?
    pub show_attachments: bool,
    /// Show all headers in message view?
    pub show_full_headers: bool,
    /// Show raw message source?
    pub show_raw: bool,
    /// Export popup visible?
    pub show_export: bool,
    /// Selected option in the export popup (0=EML, 1=TXT, 2=CSV, 3=Attachments).
    pub export_selected: usize,
    /// Selected attachment index in the attachment popup.
    pub attachment_selected: usize,

    // ── Threading ─────────────────────────────
    /// Whether threaded view is enabled.
    pub threaded_view: bool,
    /// Cached threads built from current entries.
    pub threads: Vec<threading::Thread>,
    /// Depth info for each visible row when in threaded mode.
    /// Index corresponds to `visible_indices`.
    pub thread_depths: Vec<usize>,

    // ── Sidebar / Labels ──────────────────────
    /// Whether the sidebar is visible.
    pub show_sidebar: bool,
    /// All unique labels found across messages, sorted alphabetically.
    pub all_labels: Vec<String>,
    /// Number of messages per label (parallel to `all_labels`).
    pub label_counts: Vec<usize>,
    /// Currently selected label index in the sidebar (None = "All Messages").
    pub sidebar_selected: usize,
    /// The active label filter (None = show all, Some = filter by label).
    pub active_label_filter: Option<String>,

    // ── Search ────────────────────────────────
    /// Is the search bar active (accepting input)?
    pub search_active: bool,
    /// Current search query text.
    pub search_query: String,
    /// Indices into `entries` that match the search.
    pub search_results: Vec<usize>,
    /// Current position within `search_results`.
    pub search_result_index: usize,

    // ── Sorting ───────────────────────────────
    pub sort_column: SortColumn,
    pub sort_ascending: bool,

    // ── Loaded message ────────────────────────
    /// Decoded body of the currently selected message.
    pub current_body: Option<MailBody>,

    // ── Lifecycle ─────────────────────────────
    pub should_quit: bool,
    /// Transient status message and the instant it was set.
    pub status_message: Option<(String, std::time::Instant)>,

    /// Cached viewport height for the list (set during render).
    pub list_viewport_height: usize,
}

impl App {
    /// Create a new `App` by loading (or building) the index for `mbox_path`.
    pub fn new(mbox_path: PathBuf, force_reindex: bool) -> anyhow::Result<Self> {
        Self::new_with_progress(mbox_path, force_reindex, &|_, _| {})
    }

    /// Create a new `App` with a progress callback for index loading.
    pub fn new_with_progress(
        mbox_path: PathBuf,
        force_reindex: bool,
        progress: &dyn Fn(u64, u64),
    ) -> anyhow::Result<Self> {
        let entries = builder::build_index(&mbox_path, force_reindex, Some(progress))?;
        let visible_indices: Vec<usize> = (0..entries.len()).collect();
        let store = MboxStore::open(&mbox_path)?;

        // Compute label counts from entries
        let mut label_map: BTreeMap<String, usize> = BTreeMap::new();
        for entry in &entries {
            for label in &entry.labels {
                *label_map.entry(label.clone()).or_insert(0) += 1;
            }
        }
        let all_labels: Vec<String> = label_map.keys().cloned().collect();
        let label_counts: Vec<usize> = all_labels.iter().map(|l| label_map[l]).collect();
        let has_labels = !all_labels.is_empty();

        let mut app = Self {
            mbox_path,
            entries,
            visible_indices,
            store,
            selected: 0,
            list_scroll_offset: 0,
            message_scroll_offset: 0,
            marked: HashSet::new(),
            focus: PanelFocus::MailList,
            layout: LayoutMode::HorizontalSplit,
            show_help: false,
            show_attachments: false,
            show_full_headers: false,
            show_raw: false,
            show_export: false,
            export_selected: 0,
            attachment_selected: 0,
            threaded_view: false,
            threads: Vec::new(),
            thread_depths: Vec::new(),
            show_sidebar: has_labels,
            all_labels,
            label_counts,
            sidebar_selected: 0,
            active_label_filter: None,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_result_index: 0,
            sort_column: SortColumn::Date,
            sort_ascending: false,
            current_body: None,
            should_quit: false,
            status_message: None,
            list_viewport_height: 20,
        };

        // Sort by date descending and load first message
        app.apply_sort();
        if !app.visible_indices.is_empty() {
            app.load_selected_body();
        }

        Ok(app)
    }

    /// Number of currently visible messages.
    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }

    /// The currently selected [`MailEntry`], if any.
    pub fn current_entry(&self) -> Option<&MailEntry> {
        self.visible_indices
            .get(self.selected)
            .map(|&idx| &self.entries[idx])
    }

    /// Select a message by its position in `visible_indices` and load its body.
    pub fn select_message(&mut self, index: usize) {
        if index >= self.visible_count() {
            return;
        }
        self.selected = index;
        self.message_scroll_offset = 0;
        self.load_selected_body();
    }

    /// Load the body of the currently selected message (best-effort).
    fn load_selected_body(&mut self) {
        if let Some(entry) = self.current_entry().cloned() {
            match self.store.get_message(&entry) {
                Ok(body) => self.current_body = Some(body.clone()),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load message body");
                    self.current_body = None;
                }
            }
        } else {
            self.current_body = None;
        }
    }

    /// Sort `visible_indices` according to the active column and direction.
    pub fn apply_sort(&mut self) {
        let entries = &self.entries;
        let asc = self.sort_ascending;
        let col = self.sort_column;
        self.visible_indices.sort_by(|&a, &b| {
            let cmp = match col {
                SortColumn::Date => entries[a].date.cmp(&entries[b].date),
                SortColumn::From => entries[a]
                    .from
                    .address
                    .to_lowercase()
                    .cmp(&entries[b].from.address.to_lowercase()),
                SortColumn::Subject => entries[a]
                    .subject
                    .to_lowercase()
                    .cmp(&entries[b].subject.to_lowercase()),
                SortColumn::Size => entries[a].length.cmp(&entries[b].length),
            };
            if asc {
                cmp
            } else {
                cmp.reverse()
            }
        });
    }

    /// Change the sort column (toggles direction if same column clicked again).
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = !matches!(column, SortColumn::Date);
        }
        self.apply_sort();
    }

    /// Toggle mark on the currently selected message.
    pub fn toggle_mark(&mut self) {
        if let Some(entry) = self.current_entry() {
            let offset = entry.offset;
            if self.marked.contains(&offset) {
                self.marked.remove(&offset);
            } else {
                self.marked.insert(offset);
            }
        }
    }

    /// Toggle between flat and threaded view.
    pub fn toggle_threads(&mut self) {
        self.threaded_view = !self.threaded_view;
        if self.threaded_view {
            self.rebuild_threaded_view();
            self.set_status("Threaded view enabled");
        } else {
            self.thread_depths.clear();
            self.apply_sort();
            self.set_status("Flat view enabled");
        }
        if !self.visible_indices.is_empty() {
            self.select_message(0);
        }
    }

    /// Rebuild the threaded view from current entries.
    fn rebuild_threaded_view(&mut self) {
        // Build threads from the entries matching visible_indices
        let active_entries: Vec<&MailEntry> = self
            .visible_indices
            .iter()
            .map(|&i| &self.entries[i])
            .collect();

        // Use all entries for threading (better context), then filter
        self.threads = threading::build_threads(&self.entries);

        let flat = threading::flatten_threads_to_indices(&self.threads);

        // Filter to only include currently visible entries
        let visible_set: std::collections::HashSet<usize> =
            self.visible_indices.iter().copied().collect();
        drop(active_entries); // no longer needed

        let mut new_indices = Vec::new();
        let mut new_depths = Vec::new();

        for (entry_idx, depth) in &flat {
            if visible_set.contains(entry_idx) {
                new_indices.push(*entry_idx);
                new_depths.push(*depth);
            }
        }

        self.visible_indices = new_indices;
        self.thread_depths = new_depths;
    }

    /// Get the thread depth for a visible row index (0 if not in threaded mode).
    pub fn thread_depth(&self, visible_idx: usize) -> usize {
        if self.threaded_view {
            self.thread_depths.get(visible_idx).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// Apply a label filter from the sidebar.
    /// `None` means show all messages, `Some(label)` filters to that label.
    pub fn apply_label_filter(&mut self, label: Option<String>) {
        self.active_label_filter = label.clone();
        self.search_query.clear();
        self.search_results.clear();

        match &label {
            None => {
                self.visible_indices = (0..self.entries.len()).collect();
            }
            Some(lbl) => {
                self.visible_indices = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.labels.iter().any(|l| l == lbl))
                    .map(|(i, _)| i)
                    .collect();
            }
        }
        self.apply_sort();
        if self.threaded_view {
            self.rebuild_threaded_view();
        }
        if !self.visible_indices.is_empty() {
            self.select_message(0);
        } else {
            self.current_body = None;
        }
        match &label {
            None => self.set_status("Showing all messages"),
            Some(lbl) => {
                let count = self.visible_indices.len();
                self.set_status(&format!("Label \"{lbl}\": {count} message(s)"));
            }
        }
    }

    /// Set a transient status message that auto-clears after a few seconds.
    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), std::time::Instant::now()));
    }

    /// Called every tick: clears expired status messages.
    pub fn tick(&mut self) {
        if let Some((_, when)) = &self.status_message {
            if when.elapsed().as_secs() >= 5 {
                self.status_message = None;
            }
        }
    }

    /// Execute a search using the advanced search engine.
    ///
    /// Supports field-specific queries (`from:`, `subject:`, `body:`, etc.),
    /// date/size filters, negation, and full-text body search.
    pub fn execute_search(&mut self) {
        if self.search_query.is_empty() {
            self.visible_indices = (0..self.entries.len()).collect();
            self.search_results.clear();
            self.apply_sort();
            if !self.visible_indices.is_empty() {
                self.select_message(0);
            }
            return;
        }

        match crate::search::execute(&self.mbox_path, &self.entries, &self.search_query, None) {
            Ok((_query, results)) => {
                self.search_results = results.clone();
                self.visible_indices = results;
                self.search_result_index = 0;
                self.apply_sort();

                if !self.visible_indices.is_empty() {
                    self.select_message(0);
                }

                let count = self.visible_indices.len();
                self.set_status(&format!("{count} result(s)"));
            }
            Err(e) => {
                tracing::warn!(error = %e, "Search failed");
                self.set_status(&format!("Search error: {e}"));
            }
        }
    }

    /// Ensure the selected row is visible given the current scroll offset.
    pub fn ensure_selected_visible(&mut self) {
        let vp = self.list_viewport_height.max(1);
        if self.selected < self.list_scroll_offset {
            self.list_scroll_offset = self.selected;
        } else if self.selected >= self.list_scroll_offset + vp {
            self.list_scroll_offset = self.selected.saturating_sub(vp - 1);
        }
    }
}
