//! Global application state for the TUI (the "Model" in Elm architecture).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
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

    /// First field in the modal (PageUp / Home jump target).
    pub fn first() -> Self {
        Self::Text
    }

    /// Last field in the modal (PageDown / End jump target).
    pub fn last() -> Self {
        Self::WithinResults
    }

    /// Whether this field is a horizontal selector whose value is changed with
    /// Left/Right (and j/k), rather than a focus-navigation target for those keys.
    pub fn is_selector(self) -> bool {
        matches!(self, Self::Size | Self::Label)
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
/// A single case-insensitive match found in the message body by the
/// interactive in-body search. Coordinates are body-relative: `line` is the
/// index into the body's unwrapped lines, `start`/`end` are byte offsets into
/// that line (guaranteed to sit on UTF-8 char boundaries).
#[derive(Debug, Clone, Copy)]
pub struct BodyMatch {
    /// Index of the body line (0-based, unwrapped) containing the match.
    pub line: usize,
    /// Byte offset of the match start within the line.
    pub start: usize,
    /// Byte offset of the match end within the line.
    pub end: usize,
}

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
    /// When the incremental (metadata) search was last marked dirty by a
    /// keystroke. The recompute is debounced: a burst of keystrokes coalesces
    /// into a single O(n) scan once input settles. See
    /// [`poll_incremental_search`](App::poll_incremental_search).
    search_dirty_since: Option<std::time::Instant>,
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
    ///
    /// Held as an [`Rc`] so selecting a message is a refcount bump rather than
    /// a deep copy of the (possibly multi-MB) body out of the store's LRU.
    pub current_body: Option<Rc<MailBody>>,
    /// Cached, fully-styled render of the current message view. Reused across
    /// frames while its key (message, width, view mode, in-body-search state) is
    /// unchanged, so idle frames and scrolling avoid re-sanitizing/re-styling the
    /// body and an extra full word-wrap. See [`mail_view::render`].
    pub render_cache: Option<crate::tui::widgets::mail_view::CachedRender>,

    // ── In-body search ────────────────────────
    /// Is the in-body search prompt open and capturing input?
    pub body_search_active: bool,
    /// Current in-body search query text.
    pub body_search_query: String,
    /// Matches of `body_search_query` within the current body, in reading order.
    pub body_search_matches: Vec<BodyMatch>,
    /// Index into `body_search_matches` of the currently focused match.
    pub body_search_index: usize,
    /// Bumped whenever `body_search_matches` is rebuilt (open / clear /
    /// recompute). Part of the render cache key so a change in highlight
    /// positions invalidates the cached, pre-styled body.
    pub body_search_gen: u64,
    /// Set when the focused match changes (open / type / `n` / `N`). The next
    /// render recomputes the scroll offset to bring the match into view, using
    /// the real wrapped-row geometry it has access to. Centring here would be
    /// wrong: the body wraps long lines, so a body-relative line index does not
    /// equal its on-screen row.
    pub body_search_recenter: bool,
    /// Absolute line index where the body starts within the message view,
    /// cached during render so the match-recentre logic can map a body-relative
    /// match line to a scroll offset.
    pub body_line_start: usize,

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
            search_dirty_since: None,
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
            render_cache: None,
            body_search_active: false,
            body_search_query: String::new(),
            body_search_matches: Vec::new(),
            body_search_index: 0,
            body_search_gen: 0,
            body_search_recenter: false,
            body_line_start: 0,
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
        self.body_search_clear();
        self.load_selected_body();
    }

    /// Load the body of the currently selected message (best-effort).
    fn load_selected_body(&mut self) {
        if let Some(entry) = self.current_entry().cloned() {
            match self.store.get_message(&entry) {
                // `get_message` returns an `Rc`; this is a refcount bump, not a
                // deep copy of the decoded body.
                Ok(body) => self.current_body = Some(body),
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
        let dir = |cmp: std::cmp::Ordering| if asc { cmp } else { cmp.reverse() };
        match self.sort_column {
            SortColumn::Date => self
                .visible_indices
                .sort_by(|&a, &b| dir(entries[a].date.cmp(&entries[b].date))),
            SortColumn::Size => self
                .visible_indices
                .sort_by(|&a, &b| dir(entries[a].length.cmp(&entries[b].length))),
            // Text columns lowercase each row once (decorate-sort-undecorate)
            // instead of twice per comparison — O(n) allocations rather than
            // O(n log n), which is noticeable when sorting 100k+ rows.
            col @ (SortColumn::From | SortColumn::Subject) => {
                let mut decorated: Vec<(String, usize)> = self
                    .visible_indices
                    .iter()
                    .map(|&i| {
                        let key = match col {
                            SortColumn::From => entries[i].from.address.to_lowercase(),
                            _ => entries[i].subject.to_lowercase(),
                        };
                        (key, i)
                    })
                    .collect();
                decorated.sort_by(|(ka, _), (kb, _)| dir(ka.cmp(kb)));
                self.visible_indices = decorated.into_iter().map(|(_, i)| i).collect();
            }
        }
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
        // A label change establishes a fresh scope; drop any lingering
        // "within previous results" mode so it cannot apply to a stale set.
        self.filter_within_results = false;

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
    /// (defaults to `w3m`). The HTML is sanitized (scripts, `on*` handlers
    /// and `javascript:` URLs stripped) before being written to a temp file,
    /// so a viewer such as a real browser cannot execute hostile email markup.
    /// The path is stored in `pending_html_view`; the loop performs the spawn
    /// so it can suspend/restore the terminal correctly.
    pub fn request_external_html_view(&mut self) {
        let html = match self.current_body.as_ref().and_then(|b| b.html.as_deref()) {
            Some(h) => crate::export::html::sanitize_html(h),
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
        // `create_new` refuses to follow a pre-planted symlink or clobber an
        // existing file — defends the predictable temp path on a shared /tmp.
        let write_result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, html.as_bytes()));
        if let Err(e) = write_result {
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
    ///
    /// The active scope is preserved: when "search within previous results" is
    /// on, the search stays inside the currently visible messages; otherwise an
    /// active sidebar label filter restricts it to the labelled messages.
    pub fn execute_search(&mut self) {
        // A full search supersedes any debounced incremental recompute.
        self.search_dirty_since = None;
        let restrict = self.search_restrict_set();
        self.execute_search_restricted(restrict);
    }

    /// Compute the index set a new search should be confined to, preserving the
    /// active scope.
    ///
    /// "Search within previous results" takes priority and returns the
    /// currently visible messages; failing that, an active sidebar label filter
    /// returns the messages bearing that label. Returns `None` when neither is
    /// active, meaning the search runs over the whole index.
    fn search_restrict_set(&self) -> Option<HashSet<usize>> {
        if self.filter_within_results {
            return Some(self.visible_indices.iter().copied().collect());
        }
        self.active_label_filter.as_ref().map(|label| {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.labels.iter().any(|l| l == label))
                .map(|(i, _)| i)
                .collect::<HashSet<usize>>()
        })
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
            self.visible_indices = match &restrict {
                Some(set) => {
                    let mut v: Vec<usize> = set.iter().copied().collect();
                    v.sort_unstable();
                    v
                }
                None => (0..self.entries.len()).collect(),
            };
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

    /// Debounce window before a keystroke triggers an incremental recompute.
    ///
    /// Slightly longer than the 100 ms event-loop tick so held/fast keys
    /// coalesce into one scan instead of one scan per key.
    const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

    /// Mark the incremental search as needing a recompute.
    ///
    /// The scan itself is deferred to [`poll_incremental_search`](App::poll_incremental_search)
    /// so a burst of keystrokes runs `search_metadata` once, not once per key.
    pub fn mark_search_dirty(&mut self) {
        self.search_dirty_since = Some(std::time::Instant::now());
    }

    /// Discard any pending incremental recompute.
    ///
    /// Called when the search bar closes or a full search supersedes the
    /// incremental one, so the debounce timer cannot fire against a query the
    /// user already abandoned.
    pub fn cancel_pending_incremental_search(&mut self) {
        self.search_dirty_since = None;
    }

    /// Run a pending incremental search once input has settled.
    ///
    /// Called once per event-loop tick. Does nothing until the debounce window
    /// has elapsed since the last keystroke.
    pub fn poll_incremental_search(&mut self) {
        if let Some(since) = self.search_dirty_since {
            if since.elapsed() >= Self::SEARCH_DEBOUNCE {
                self.search_dirty_since = None;
                self.execute_incremental_search();
            }
        }
    }

    /// Run a fast metadata-only incremental search.
    ///
    /// Normally reached via the debounced [`poll_incremental_search`](App::poll_incremental_search),
    /// but callable directly (tests, programmatic refresh). If the query
    /// requires full-text search (body:), this skips filtering and shows all
    /// entries (full search runs on Enter).
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

        // Single metadata pass. `search_metadata` already returns the matching
        // entry indices in ascending order, so we use them directly instead of
        // building a `(0..n)` base set and re-filtering it through a `HashSet`
        // (the old double pass + extra allocation on every keystroke). When a
        // sidebar label filter is active, narrow the results in one `retain`.
        let mut results = crate::search::metadata::search_metadata(&self.entries, &query);
        if let Some(ref label) = self.active_label_filter {
            results.retain(|&i| self.entries[i].labels.iter().any(|l| l == label));
        }
        self.visible_indices = results;

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

    /// Reset the search filter popup form fields to their defaults.
    ///
    /// `filter_within_results` is deliberately *not* cleared here: it is a
    /// scoping mode, not a one-shot field, so it must survive reopening the
    /// popup (see #11). It is dropped instead when the underlying scope is
    /// reset — e.g. on a label-filter change or when leaving the search.
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

impl App {
    /// Open the interactive in-body search prompt, starting from a blank query.
    pub fn body_search_open(&mut self) {
        self.body_search_active = true;
        self.body_search_query.clear();
        self.body_search_matches.clear();
        self.body_search_index = 0;
        self.body_search_gen = self.body_search_gen.wrapping_add(1);
        self.body_search_recenter = false;
    }

    /// Clear all in-body search state (query, matches, prompt).
    pub fn body_search_clear(&mut self) {
        self.body_search_active = false;
        self.body_search_query.clear();
        self.body_search_matches.clear();
        self.body_search_index = 0;
        self.body_search_gen = self.body_search_gen.wrapping_add(1);
        self.body_search_recenter = false;
    }

    /// Recompute the list of matches for the current query against the body
    /// text. Resets the focused match to the first hit and scrolls to it.
    pub fn recompute_body_matches(&mut self) {
        self.body_search_matches.clear();
        self.body_search_index = 0;
        self.body_search_gen = self.body_search_gen.wrapping_add(1);

        if self.body_search_query.is_empty() {
            return;
        }
        let Some(body) = &self.current_body else {
            return;
        };
        let Some(text) = &body.text else {
            return;
        };

        for (line_idx, line) in text.lines().enumerate() {
            // Sanitize exactly like the renderer does, so the byte ranges
            // recorded here line up with the displayed (sanitized) text.
            let line = crate::tui::text::sanitize_line(line);
            for (start, end) in find_matches_ci(&line, &self.body_search_query) {
                self.body_search_matches.push(BodyMatch {
                    line: line_idx,
                    start,
                    end,
                });
            }
        }
        self.body_search_recenter = true;
    }

    /// Move the focus to the next match (wrapping around) and request a scroll
    /// that brings it into view on the next render.
    pub fn body_search_next(&mut self) {
        if self.body_search_matches.is_empty() {
            return;
        }
        self.body_search_index = (self.body_search_index + 1) % self.body_search_matches.len();
        self.body_search_recenter = true;
    }

    /// Move the focus to the previous match (wrapping around) and request a
    /// scroll that brings it into view on the next render.
    pub fn body_search_prev(&mut self) {
        if self.body_search_matches.is_empty() {
            return;
        }
        let len = self.body_search_matches.len();
        self.body_search_index = (self.body_search_index + len - 1) % len;
        self.body_search_recenter = true;
    }
}

/// Find every case-insensitive, non-overlapping occurrence of `needle` in
/// `haystack`, returning `(start, end)` byte ranges into `haystack`. Both ends
/// land on UTF-8 char boundaries, so the ranges are always safe to slice.
/// Comparison is Unicode-aware (`char::to_lowercase`) and tolerant of the rare
/// case where lowercasing changes a character's byte width.
fn find_matches_ci(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    if needle.is_empty() {
        return matches;
    }

    let hay: Vec<(usize, char)> = haystack.char_indices().collect();
    let need: Vec<char> = needle.chars().collect();
    let mut i = 0;
    while i + need.len() <= hay.len() {
        let is_match = (0..need.len()).all(|j| chars_eq_ci(hay[i + j].1, need[j]));
        if is_match {
            let start = hay[i].0;
            let end = hay
                .get(i + need.len())
                .map(|&(byte, _)| byte)
                .unwrap_or(haystack.len());
            matches.push((start, end));
            i += need.len(); // non-overlapping
        } else {
            i += 1;
        }
    }
    matches
}

/// Compare two characters ignoring case, covering ASCII and common Unicode.
fn chars_eq_ci(a: char, b: char) -> bool {
    a == b || a.eq_ignore_ascii_case(&b) || a.to_lowercase().eq(b.to_lowercase())
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
mod body_search_tests {
    use super::{find_matches_ci, App, BodyMatch};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn find_matches_basic_and_case_insensitive() {
        let hits = find_matches_ci("The Audit and the audit AUDIT", "audit");
        assert_eq!(hits.len(), 3);
        // Each range slices back to a case-insensitive copy of the needle.
        for (s, e) in hits {
            assert_eq!(
                "The Audit and the audit AUDIT"[s..e].to_lowercase(),
                "audit"
            );
        }
    }

    #[test]
    fn find_matches_non_overlapping() {
        // "aa" in "aaaa" yields two non-overlapping matches, not three.
        assert_eq!(find_matches_ci("aaaa", "aa"), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn find_matches_empty_needle_is_empty() {
        assert!(find_matches_ci("anything", "").is_empty());
    }

    #[test]
    fn find_matches_unicode_boundaries() {
        // Ensure byte ranges land on char boundaries for multi-byte text.
        let hay = "café Café CAFÉ";
        let hits = find_matches_ci(hay, "café");
        assert_eq!(hits.len(), 3);
        for (s, e) in hits {
            assert_eq!(hay[s..e].to_lowercase(), "café");
        }
    }

    #[test]
    fn external_html_view_sanitizes_script() {
        use super::MailBody;
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.current_body = Some(std::rc::Rc::new(MailBody {
            text: Some("plain".to_string()),
            html: Some(
                "<p>ok</p><script>alert(1)</script><a href=\"javascript:x\">l</a>".to_string(),
            ),
            raw_headers: String::new(),
            attachments: vec![],
        }));
        app.request_external_html_view();
        let path = app
            .pending_html_view
            .clone()
            .expect("temp path should be set");
        let content = std::fs::read_to_string(&path).expect("temp file readable");
        assert!(content.contains("ok"), "safe content must be preserved");
        assert!(!content.contains("<script"), "script tag must be stripped");
        assert!(
            !content.contains("javascript:"),
            "javascript: URL must be stripped"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.body_search_matches = vec![
            BodyMatch {
                line: 0,
                start: 0,
                end: 1,
            },
            BodyMatch {
                line: 1,
                start: 0,
                end: 1,
            },
            BodyMatch {
                line: 2,
                start: 0,
                end: 1,
            },
        ];
        app.body_search_index = 0;

        app.body_search_next();
        assert_eq!(app.body_search_index, 1);
        assert!(
            app.body_search_recenter,
            "moving to a match requests a recentre on the next render"
        );
        app.body_search_next();
        app.body_search_next();
        assert_eq!(app.body_search_index, 0, "next wraps past the end");

        app.body_search_prev();
        assert_eq!(app.body_search_index, 2, "prev wraps before the start");
        assert!(app.body_search_recenter);
    }

    #[test]
    fn next_prev_on_empty_matches_do_not_request_recenter() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.body_search_matches.clear();
        app.body_search_recenter = false;
        app.body_search_next();
        app.body_search_prev();
        assert!(
            !app.body_search_recenter,
            "no matches means nothing to scroll to"
        );
    }

    #[test]
    fn clear_resets_all_state() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.body_search_active = true;
        app.body_search_query = "x".to_string();
        app.body_search_matches = vec![BodyMatch {
            line: 0,
            start: 0,
            end: 1,
        }];
        app.body_search_index = 0;

        app.body_search_recenter = true;
        app.body_search_clear();
        assert!(!app.body_search_active);
        assert!(app.body_search_query.is_empty());
        assert!(app.body_search_matches.is_empty());
        assert_eq!(app.body_search_index, 0);
        assert!(!app.body_search_recenter);
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

    #[test]
    fn search_respects_active_label_filter_scope() {
        use std::sync::Arc;
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");

        // Tag the first two entries with a label and activate the label filter,
        // mirroring what apply_label_filter does in the TUI.
        {
            let entries = Arc::make_mut(&mut app.entries);
            entries[0].labels.push("Inbox".to_string());
            entries[1].labels.push("Inbox".to_string());
        }
        app.active_label_filter = Some("Inbox".to_string());
        app.visible_indices = vec![0, 1];

        // A metadata-only query that would match more than the scoped subset.
        // With the bug, results contained every matching sender across the whole
        // index; the fix restricts them to the active label scope.
        app.search_query = "from:user".to_string();
        app.execute_search();

        assert!(
            !app.search_in_progress(),
            "metadata-only search must not spawn a worker"
        );
        assert!(
            app.visible_indices.iter().all(|i| *i < 2),
            "search results must stay within the active label scope, got {:?}",
            app.visible_indices,
        );
    }

    #[test]
    fn empty_query_respects_active_label_filter_scope() {
        use std::sync::Arc;
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");

        {
            let entries = Arc::make_mut(&mut app.entries);
            entries[2].labels.push("Work".to_string());
        }
        app.active_label_filter = Some("Work".to_string());
        app.visible_indices = vec![2];

        // Pressing Enter on an empty query should leave the scope intact rather
        // than fall back to "all entries".
        app.search_query.clear();
        app.execute_search();

        assert_eq!(app.visible_indices, vec![2]);
    }

    /// Regression for #11: "search within previous results" must survive
    /// reopening the filter popup and must scope a subsequent body search to
    /// the previously matched messages, instead of searching the whole index.
    #[test]
    fn within_results_persists_and_scopes_body_search() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");

        // Step 1: a subject search narrows the view to the "Hello World" thread.
        app.search_query = "subject:Hello".to_string();
        app.execute_search();
        assert!(!app.search_in_progress(), "metadata search runs inline");
        let mut scoped = app.visible_indices.clone();
        scoped.sort_unstable();
        assert_eq!(scoped, vec![0, 1]);

        // Step 2: the user enables "search within previous results".
        app.filter_within_results = true;

        // Step 3: reopening the popup resets the form fields, but the
        // within-results mode must persist (this is the #11 regression).
        app.reset_search_filters();
        assert!(
            app.filter_within_results,
            "within-results mode must survive reopening the filter popup"
        );

        // Step 4: a body/free-text search. "This" matches the bodies of
        // messages 0, 1 and 3; scoped to the previous {0, 1} it must never
        // leak message 3.
        app.filter_text = "This".to_string();
        app.search_query = app.build_query_from_filters();
        app.execute_search();
        drain_search(&mut app);

        assert!(
            app.visible_indices.iter().all(|i| scoped.contains(i)),
            "body search must stay within previous results, got {:?}",
            app.visible_indices
        );
        let mut result = app.visible_indices.clone();
        result.sort_unstable();
        assert_eq!(result, vec![0, 1]);
    }

    /// Backing out of a scope (changing the sidebar label filter) must drop the
    /// within-results mode so it cannot silently apply to a stale result set.
    #[test]
    fn within_results_clears_on_label_filter_change() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.filter_within_results = true;
        app.apply_label_filter(None);
        assert!(
            !app.filter_within_results,
            "changing the label scope must drop within-results mode"
        );
    }

    /// Selecting a message must share the decoded body with the store's cache
    /// via `Rc`, not deep-copy it. A strong count ≥ 2 (store + `current_body`)
    /// proves the body was not cloned out of the LRU on selection.
    #[test]
    fn load_body_is_shared_not_deep_copied() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        assert!(!app.visible_indices.is_empty(), "fixture has messages");
        app.select_message(0);
        let body = app
            .current_body
            .as_ref()
            .expect("selecting a valid message loads its body");
        assert!(
            std::rc::Rc::strong_count(body) >= 2,
            "body should be shared (store LRU + current_body), got {}",
            std::rc::Rc::strong_count(body)
        );
    }

    /// The incremental search runs a single metadata pass: `search_metadata`'s
    /// output is used directly (no `(0..n)` base set + HashSet re-filter), and
    /// an active label filter narrows it in one `retain`.
    #[test]
    fn incremental_search_single_pass() {
        use std::sync::Arc;
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");

        // No label filter: results are exactly the metadata matches.
        app.search_query = "from:user1".to_string();
        app.execute_incremental_search();
        assert_eq!(
            app.visible_indices,
            vec![0],
            "from:user1 matches only the first message"
        );

        // With an active label filter, results are the metadata matches
        // intersected with the labelled subset — narrowed in one pass.
        {
            let entries = Arc::make_mut(&mut app.entries);
            entries[0].labels.push("Inbox".to_string());
            entries[1].labels.push("Inbox".to_string());
        }
        app.active_label_filter = Some("Inbox".to_string());
        app.search_query = "from:user".to_string();
        app.execute_incremental_search();
        let mut got = app.visible_indices.clone();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![0, 1],
            "from:user within the Inbox label keeps only labelled matches"
        );
    }

    /// Keystrokes debounce: `mark_search_dirty` schedules a recompute that
    /// `poll_incremental_search` only runs after the window elapses, and
    /// `cancel_pending_incremental_search` discards it.
    #[test]
    fn incremental_search_is_debounced_and_cancelable() {
        let mut app = App::new(fixture("simple.mbox"), true).expect("open fixture");
        app.search_query = "from:user1".to_string();

        app.mark_search_dirty();
        // Polling immediately (well within the debounce window) must not fire.
        app.poll_incremental_search();
        assert!(
            app.search_dirty_since.is_some(),
            "a poll before the debounce window keeps the recompute pending"
        );

        app.cancel_pending_incremental_search();
        assert!(
            app.search_dirty_since.is_none(),
            "cancel must discard the pending recompute"
        );
        // A poll with nothing pending is a no-op.
        app.poll_incremental_search();
    }
}
