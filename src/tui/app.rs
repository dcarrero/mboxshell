//! Global application state for the TUI (the "Model" in Elm architecture).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use crate::i18n;
use crate::index::builder;
use crate::model::mail::{MailBody, MailEntry};
use crate::store::reader::MboxStore;
use crate::tui::threading;

/// Shared progress counters for an in-flight background search.
#[derive(Default)]
pub struct SearchProgress {
    /// Number of candidate messages whose body has been scanned so far.
    pub processed: AtomicUsize,
    /// Total number of candidate messages to scan.
    pub total: AtomicUsize,
}

/// A full-text search running on a background thread.
///
/// Body scans read every candidate message from disk, which can take a while
/// on large MBOX files. Running them off the UI thread keeps the interface
/// responsive and cancelable (Esc), satisfying the "all I/O must be cancelable"
/// rule. The main loop polls [`App::poll_search`] each tick to collect results.
pub struct SearchJob {
    /// Receives the final result (or error) from the worker thread.
    rx: Receiver<crate::error::Result<Vec<usize>>>,
    /// Set to `true` to ask the worker to stop early.
    cancel: Arc<AtomicBool>,
    /// Live progress counters updated by the worker.
    progress: Arc<SearchProgress>,
    /// Optional set the results must be intersected with ("search within
    /// previous results").
    restrict: Option<HashSet<usize>>,
}

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

/// Field currently focused in the search filter popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFilterField {
    Text,
    From,
    To,
    Subject,
    DateFrom,
    DateTo,
    Size,
    HasAttachment,
    Label,
    WithinResults,
}

impl SearchFilterField {
    /// Advance to the next field (Tab). `has_labels` controls whether the Label row is shown.
    pub fn next(self, has_labels: bool) -> Self {
        match self {
            Self::Text => Self::From,
            Self::From => Self::To,
            Self::To => Self::Subject,
            Self::Subject => Self::DateFrom,
            Self::DateFrom => Self::DateTo,
            Self::DateTo => Self::Size,
            Self::Size => Self::HasAttachment,
            Self::HasAttachment => {
                if has_labels {
                    Self::Label
                } else {
                    Self::WithinResults
                }
            }
            Self::Label => Self::WithinResults,
            Self::WithinResults => Self::Text,
        }
    }

    /// Move to the previous field (Shift-Tab).
    pub fn prev(self, has_labels: bool) -> Self {
        match self {
            Self::Text => Self::WithinResults,
            Self::From => Self::Text,
            Self::To => Self::From,
            Self::Subject => Self::To,
            Self::DateFrom => Self::Subject,
            Self::DateTo => Self::DateFrom,
            Self::Size => Self::DateTo,
            Self::HasAttachment => Self::Size,
            Self::Label => Self::HasAttachment,
            Self::WithinResults => {
                if has_labels {
                    Self::Label
                } else {
                    Self::HasAttachment
                }
            }
        }
    }

    /// Whether this field accepts free-form text input.
    pub fn is_text_input(self) -> bool {
        matches!(
            self,
            Self::Text | Self::From | Self::To | Self::Subject | Self::DateFrom | Self::DateTo
        )
    }
}

/// Size filter options for the search filter popup.
pub const SIZE_OPTIONS: &[(&str, &str)] = &[
    ("Any", ""),
    ("> 100 KB", "size:>100kb"),
    ("> 1 MB", "size:>1mb"),
    ("> 5 MB", "size:>5mb"),
    ("> 10 MB", "size:>10mb"),
    ("< 10 KB", "size:<10kb"),
];

/// Maximum number of entries kept in search history.
const MAX_SEARCH_HISTORY: usize = 20;

/// Complete TUI state.
pub struct App {
    // ── Data ──────────────────────────────────
    /// Path to the open MBOX file.
    pub mbox_path: PathBuf,
    /// Full index of messages (in memory). Wrapped in `Arc` so background
    /// search threads can share it without copying.
    pub entries: Arc<Vec<MailEntry>>,
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
    /// In-flight background full-text search, if any.
    pub search_job: Option<SearchJob>,

    // ── Search filter popup ──────────────────
    /// Whether the search filter popup is visible.
    pub show_search_filter: bool,
    /// Which field is focused in the filter popup.
    pub search_filter_focus: SearchFilterField,
    /// Text field value in the filter popup.
    pub filter_text: String,
    /// From field value in the filter popup.
    pub filter_from: String,
    /// To field value in the filter popup.
    pub filter_to: String,
    /// Subject field value in the filter popup.
    pub filter_subject: String,
    /// Date-from field value in the filter popup.
    pub filter_date_from: String,
    /// Date-to field value in the filter popup.
    pub filter_date_to: String,
    /// Selected index in the size selector (into `SIZE_OPTIONS`).
    pub filter_size_selected: usize,
    /// Whether the "has attachment" checkbox is checked.
    pub filter_has_attachment: bool,
    /// Selected index in the label selector (0 = Any, 1..N = labels).
    pub filter_label_selected: usize,
    /// Whether the "search within previous results" checkbox is checked.
    pub filter_within_results: bool,

    // ── Search history ───────────────────────
    /// Recent search queries, most recent first.
    pub search_history: Vec<String>,
    /// Current position in history when navigating with Up/Down (None = not navigating).
    pub search_history_index: Option<usize>,
    /// Draft query saved when the user starts navigating history.
    pub search_draft: String,

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
    /// Cached viewport height for the message view (set during render).
    pub message_view_height: usize,

    /// Pending external HTML view request. When `Some`, the main loop should
    /// suspend the TUI, run the configured viewer on the temp file, then resume.
    pub pending_html_view: Option<PathBuf>,
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

        let entries = Arc::new(entries);

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
            search_job: None,
            show_search_filter: false,
            search_filter_focus: SearchFilterField::Text,
            filter_text: String::new(),
            filter_from: String::new(),
            filter_to: String::new(),
            filter_subject: String::new(),
            filter_date_from: String::new(),
            filter_date_to: String::new(),
            filter_size_selected: 0,
            filter_has_attachment: false,
            filter_label_selected: 0,
            filter_within_results: false,
            search_history: Vec::new(),
            search_history_index: None,
            search_draft: String::new(),
            sort_column: SortColumn::Date,
            sort_ascending: false,
            current_body: None,
            should_quit: false,
            status_message: None,
            list_viewport_height: 20,
            message_view_height: 20,
            pending_html_view: None,
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
            self.set_status(i18n::tui_threaded_view());
        } else {
            self.thread_depths.clear();
            self.apply_sort();
            self.set_status(i18n::tui_flat_view());
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
            None => self.set_status(i18n::tui_showing_all()),
            Some(lbl) => {
                let count = self.visible_indices.len();
                self.set_status(&format!(
                    "Label \"{lbl}\": {count} {}",
                    i18n::tui_messages_count()
                ));
            }
        }
    }

    /// Set a transient status message that auto-clears after a few seconds.
    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), std::time::Instant::now()));
    }

    /// Request the main loop to open the current message's HTML body in
    /// the external viewer configured via `MBOXSHELL_HTML_VIEWER`
    /// (defaults to `w3m`). Writes the HTML to a temp file and stores
    /// the path in `pending_html_view`; the loop performs the spawn so
    /// it can suspend/restore the terminal correctly.
    pub fn request_external_html_view(&mut self) {
        let html = match self.current_body.as_ref().and_then(|b| b.html.as_deref()) {
            Some(h) => h.to_string(),
            None => {
                self.set_status(i18n::tui_no_html_part());
                return;
            }
        };
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!("mboxshell-{}-{}.html", std::process::id(), stamp));
        if let Err(e) = std::fs::write(&path, html) {
            self.set_status(&format!("{}: {e}", i18n::tui_export_error()));
            return;
        }
        self.pending_html_view = Some(path);
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
        self.execute_search_restricted(None);
    }

    /// Like [`execute_search`](Self::execute_search), but intersects the
    /// results with `restrict` ("search within previous results").
    ///
    /// Metadata-only queries run synchronously (they are instant). Queries
    /// that need a body scan are dispatched to a background thread via
    /// [`spawn_search_job`](Self::spawn_search_job) so the UI stays responsive
    /// and cancelable; their results are applied later by
    /// [`poll_search`](Self::poll_search).
    pub fn execute_search_restricted(&mut self, restrict: Option<HashSet<usize>>) {
        // Drop any in-flight search before starting a new one.
        self.cancel_search();

        if self.search_query.is_empty() {
            self.visible_indices = (0..self.entries.len()).collect();
            self.search_results.clear();
            self.apply_sort();
            if !self.visible_indices.is_empty() {
                self.select_message(0);
            }
            return;
        }

        let query = crate::search::query::parse_query(&self.search_query);

        if crate::search::needs_body_scan(&query) {
            self.spawn_search_job(restrict);
            return;
        }

        // Metadata-only: fast enough to run inline.
        match crate::search::execute(&self.mbox_path, &self.entries, &self.search_query, None) {
            Ok((_query, results)) => self.apply_search_results(results, restrict),
            Err(e) => {
                tracing::warn!(error = %e, "Search failed");
                self.set_status(&format!("{}: {e}", i18n::tui_search_error()));
            }
        }
    }

    /// Spawn a background thread to run the current query's body scan.
    ///
    /// The worker shares the index via `Arc`, reports progress through shared
    /// atomics, and stops early when `cancel` is set. The TUI keeps running
    /// and shows live progress; results are collected in
    /// [`poll_search`](Self::poll_search).
    fn spawn_search_job(&mut self, restrict: Option<HashSet<usize>>) {
        let entries = Arc::clone(&self.entries);
        let path = self.mbox_path.clone();
        let query_str = self.search_query.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(SearchProgress::default());
        let (tx, rx) = mpsc::channel();

        let cancel_worker = Arc::clone(&cancel);
        let progress_worker = Arc::clone(&progress);
        std::thread::spawn(move || {
            let on_progress = move |done: usize, total: usize| {
                progress_worker.processed.store(done, Ordering::Relaxed);
                progress_worker.total.store(total, Ordering::Relaxed);
                !cancel_worker.load(Ordering::Relaxed)
            };
            let result = crate::search::execute(&path, &entries, &query_str, Some(&on_progress))
                .map(|(_, results)| results);
            // The receiver may already be gone (search cancelled/superseded);
            // ignoring the send error is intentional.
            let _ = tx.send(result);
        });

        self.search_job = Some(SearchJob {
            rx,
            cancel,
            progress,
            restrict,
        });
        self.set_status(&format!(
            "{} ({})",
            i18n::tui_searching(),
            i18n::tui_search_cancel_hint()
        ));
    }

    /// Poll the in-flight background search (called once per event-loop tick).
    ///
    /// Applies results when the worker finishes, refreshes the progress status
    /// while it runs, and clears the job if the worker thread vanished.
    pub fn poll_search(&mut self) {
        let outcome = match &self.search_job {
            Some(job) => job.rx.try_recv(),
            None => return,
        };

        match outcome {
            Ok(result) => {
                if let Some(job) = self.search_job.take() {
                    match result {
                        Ok(results) => self.apply_search_results(results, job.restrict),
                        Err(e) => {
                            tracing::warn!(error = %e, "Search failed");
                            self.set_status(&format!("{}: {e}", i18n::tui_search_error()));
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                // Still running — refresh the live progress line.
                let (done, total) = match &self.search_job {
                    Some(job) => (
                        job.progress.processed.load(Ordering::Relaxed),
                        job.progress.total.load(Ordering::Relaxed),
                    ),
                    None => return,
                };
                let msg = if total > 0 {
                    format!(
                        "{} {done}/{total} ({})",
                        i18n::tui_searching(),
                        i18n::tui_search_cancel_hint()
                    )
                } else {
                    format!(
                        "{} ({})",
                        i18n::tui_searching(),
                        i18n::tui_search_cancel_hint()
                    )
                };
                self.set_status(&msg);
            }
            Err(TryRecvError::Disconnected) => {
                self.search_job = None;
            }
        }
    }

    /// Whether a background search is currently running.
    pub fn search_in_progress(&self) -> bool {
        self.search_job.is_some()
    }

    /// Cancel any in-flight background search, signalling the worker to stop.
    ///
    /// The visible list is left untouched (the pre-search view stays on
    /// screen). Reports a cancellation status only when a search was actually
    /// running.
    pub fn cancel_search(&mut self) {
        if let Some(job) = self.search_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            self.set_status(i18n::tui_search_cancelled());
        }
    }

    /// Apply a finished result set to the visible list, optionally intersecting
    /// it with `restrict` (the "search within previous results" set).
    fn apply_search_results(&mut self, mut results: Vec<usize>, restrict: Option<HashSet<usize>>) {
        if let Some(prev) = &restrict {
            results.retain(|i| prev.contains(i));
        }
        self.search_results = results.clone();
        self.visible_indices = results;
        self.search_result_index = 0;
        self.apply_sort();
        if self.threaded_view {
            self.rebuild_threaded_view();
        }
        if !self.visible_indices.is_empty() {
            self.select_message(0);
        } else {
            self.current_body = None;
        }
        let count = self.visible_indices.len();
        self.set_status(&format!("{count} {}", i18n::tui_results()));
    }

    /// Run a fast metadata-only incremental search (called on each keystroke).
    ///
    /// If the query requires full-text search (body:), this skips filtering
    /// and shows all entries (full search runs on Enter).
    pub fn execute_incremental_search(&mut self) {
        if self.search_query.is_empty() {
            // Restore all messages (respecting label filter)
            if let Some(label) = self.active_label_filter.clone() {
                self.visible_indices = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.labels.iter().any(|l| l == &label))
                    .map(|(i, _)| i)
                    .collect();
            } else {
                self.visible_indices = (0..self.entries.len()).collect();
            }
            self.apply_sort();
            if !self.visible_indices.is_empty() {
                self.select_message(0);
            }
            return;
        }

        let query = crate::search::query::parse_query(&self.search_query);

        // Skip incremental filtering if full-text is needed (too slow)
        if query.needs_fulltext {
            return;
        }

        // Start from full set (or label-filtered set)
        let base_indices: Vec<usize> = if let Some(ref label) = self.active_label_filter {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.labels.iter().any(|l| l == label))
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.entries.len()).collect()
        };

        // Filter using metadata search
        let all_results = crate::search::metadata::search_metadata(&self.entries, &query);
        let result_set: std::collections::HashSet<usize> = all_results.into_iter().collect();
        self.visible_indices = base_indices
            .into_iter()
            .filter(|i| result_set.contains(i))
            .collect();

        self.apply_sort();
        if self.threaded_view {
            self.rebuild_threaded_view();
        }
        if !self.visible_indices.is_empty() {
            self.select_message(0);
        } else {
            self.current_body = None;
        }
    }

    /// Build a query string from the current filter popup fields.
    pub fn build_query_from_filters(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !self.filter_text.is_empty() {
            // Free-text: pushed verbatim (not quoted) so each word becomes its
            // own AND term. "multi word search" then matches messages that
            // contain all three words, rather than that exact contiguous
            // phrase. Field values below stay quoted to survive tokenization.
            parts.push(self.filter_text.trim().to_string());
        }
        if !self.filter_from.is_empty() {
            parts.push(format!("from:{}", quote_if_needed(&self.filter_from)));
        }
        if !self.filter_to.is_empty() {
            parts.push(format!("to:{}", quote_if_needed(&self.filter_to)));
        }
        if !self.filter_subject.is_empty() {
            parts.push(format!("subject:{}", quote_if_needed(&self.filter_subject)));
        }

        // Date range
        let has_from = !self.filter_date_from.is_empty();
        let has_to = !self.filter_date_to.is_empty();
        if has_from && has_to {
            parts.push(format!(
                "date:{}..{}",
                self.filter_date_from, self.filter_date_to
            ));
        } else if has_from {
            parts.push(format!("after:{}", self.filter_date_from));
        } else if has_to {
            parts.push(format!("before:{}", self.filter_date_to));
        }

        // Size selector
        if self.filter_size_selected > 0 {
            if let Some(&(_, query_part)) = SIZE_OPTIONS.get(self.filter_size_selected) {
                if !query_part.is_empty() {
                    parts.push(query_part.to_string());
                }
            }
        }

        if self.filter_has_attachment {
            parts.push("has:attachment".to_string());
        }

        // Label selector (0 = Any, skip)
        if self.filter_label_selected > 0 {
            if let Some(label) = self.all_labels.get(self.filter_label_selected - 1) {
                parts.push(format!("label:{}", quote_if_needed(label)));
            }
        }

        parts.join(" ")
    }

    /// Reset all search filter popup fields to their defaults.
    pub fn reset_search_filters(&mut self) {
        self.search_filter_focus = SearchFilterField::Text;
        self.filter_text.clear();
        self.filter_from.clear();
        self.filter_to.clear();
        self.filter_subject.clear();
        self.filter_date_from.clear();
        self.filter_date_to.clear();
        self.filter_size_selected = 0;
        self.filter_has_attachment = false;
        self.filter_label_selected = 0;
        self.filter_within_results = false;
    }

    /// Push a query into the search history (most recent first, dedup, capped).
    pub fn push_search_history(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        let q = query.to_string();
        // Remove duplicates
        self.search_history.retain(|h| h != &q);
        // Insert at front
        self.search_history.insert(0, q);
        // Cap at max
        self.search_history.truncate(MAX_SEARCH_HISTORY);
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

/// Quote a filter value if it contains whitespace, so it survives query
/// tokenization as a single phrase. Values that already contain a double
/// quote are returned as-is to avoid producing malformed queries.
fn quote_if_needed(value: &str) -> String {
    if value.contains('"') || !value.chars().any(char::is_whitespace) {
        value.to_string()
    } else {
        format!("\"{value}\"")
    }
}

#[cfg(test)]
mod build_query_tests {
    use super::quote_if_needed;

    #[test]
    fn quote_if_needed_no_spaces() {
        assert_eq!(quote_if_needed("hello"), "hello");
    }

    #[test]
    fn quote_if_needed_with_spaces() {
        assert_eq!(quote_if_needed("monthly report"), "\"monthly report\"");
    }

    #[test]
    fn quote_if_needed_preexisting_quote_passthrough() {
        // Already quoted or contains a quote — don't re-wrap.
        assert_eq!(quote_if_needed("\"already\""), "\"already\"");
    }
}

#[cfg(test)]
mod async_search_tests {
    use super::App;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    /// Drive an in-flight background search to completion (bounded wait).
    fn drain_search(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.search_in_progress() {
            app.poll_search();
            assert!(
                Instant::now() < deadline,
                "background search did not finish in time"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn multiword_body_search_runs_in_background_and_applies_results() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_query = "perspective message".to_string();
        app.execute_search();

        // A free-text body scan is dispatched to a worker, not applied inline.
        assert!(
            app.search_in_progress(),
            "free-text search should run in the background"
        );

        drain_search(&mut app);

        let subjects: Vec<String> = app
            .visible_indices
            .iter()
            .map(|&i| app.entries[i].subject.clone())
            .collect();
        assert_eq!(subjects, vec!["Message with From in body".to_string()]);
    }

    #[test]
    fn metadata_only_search_runs_inline() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_query = "from:user1".to_string();
        app.execute_search();
        assert!(
            !app.search_in_progress(),
            "metadata-only search must not spawn a background job"
        );
    }

    #[test]
    fn cancelling_a_search_clears_the_job() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_query = "perspective".to_string();
        app.execute_search();
        assert!(app.search_in_progress());
        app.cancel_search();
        assert!(
            !app.search_in_progress(),
            "cancel_search must drop the in-flight job"
        );
    }
}
