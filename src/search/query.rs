//! Search query parser.
//!
//! Parses user-typed query strings into a structured [`SearchQuery`].
//!
//! # Supported syntax
//!
//! **Simple search**: `texto` — searches in subject, from, to (metadata).
//!
//! **Field-specific**:
//! - `from:user@example.com`
//! - `to:dest@example.com`
//! - `cc:copy@example.com`
//! - `subject:invoice`
//! - `body:important text`  (triggers full-text search)
//! - `has:attachment` / `has:no-attachment`
//! - `label:inbox`
//! - `filename:report.pdf`
//! - `id:<message-id@domain>`
//!
//! **Date filters**:
//! - `date:2024-01-01` / `date:2024-01` / `date:2024`
//! - `date:2024-01-01..2024-06-30`
//! - `before:2024-06-01` / `after:2024-01-01`
//!
//! **Size filters**:
//! - `size:>1mb` / `size:<100kb`
//!
//! **Operators**:
//! - `term1 term2` — implicit AND
//! - `term1 OR term2` — explicit OR
//! - `-term` — NOT (exclude)
//! - `"exact phrase"` — quoted phrase
//!
//! `OR` binds tighter than the implicit AND, so
//! `from:alice OR from:bob subject:invoice` reads as
//! `(from:alice OR from:bob) AND subject:invoice`. There are no parentheses:
//! a query is a list of AND-ed groups, each group a list of OR-ed terms.
//!
//! `OR` only joins *terms*. Date, size and attachment filters are always
//! AND-ed, so an `OR` next to one of them (`from:a OR has:attachment`) leaves
//! the filter as a plain AND condition.

use chrono::NaiveDate;

/// Which field to search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchField {
    /// Search in subject + from + to (default).
    All,
    From,
    To,
    Cc,
    Subject,
    Body,
    Label,
    Filename,
    MessageId,
}

/// How to match text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOperator {
    /// Case-insensitive substring match.
    Contains(String),
    /// Exact quoted phrase (still case-insensitive).
    Exact(String),
}

/// Date range filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateFilter {
    /// Single day.
    Exact(NaiveDate),
    /// Inclusive range.
    Range(NaiveDate, NaiveDate),
    /// Strictly before a date; the day itself does not match.
    Before(NaiveDate),
    /// On or after a date; the day itself matches.
    After(NaiveDate),
    /// All days in a month.
    Month(i32, u32),
    /// All days in a year.
    Year(i32),
}

/// Size comparison filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeFilter {
    GreaterThan(u64),
    LessThan(u64),
}

/// A single search term.
#[derive(Debug, Clone)]
pub struct SearchTerm {
    pub field: SearchField,
    pub operator: SearchOperator,
    pub negated: bool,
}

/// One or more terms joined by `OR`. An entry satisfies the group when it
/// matches **any** of them.
///
/// A group of one is the common case — a term with no `OR` beside it.
#[derive(Debug, Clone)]
pub struct TermGroup {
    pub terms: Vec<SearchTerm>,
}

impl TermGroup {
    /// Whether any term in the group has to read the message body.
    ///
    /// `All` (free-text) counts: it matches metadata *or* body, so the group
    /// cannot be settled without the body unless it already matched.
    pub fn needs_body(&self) -> bool {
        self.terms.iter().any(|t| {
            matches!(
                t.field,
                SearchField::Body | SearchField::Filename | SearchField::All
            )
        })
    }
}

/// A fully parsed search query.
///
/// The term groups are AND-ed together and each group is OR-ed internally, so
/// `a OR b c` is `(a OR b) AND c`. Date, size and attachment filters apply on
/// top of all of them.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Term groups, AND-ed together.
    pub groups: Vec<TermGroup>,
    /// Date filters, AND-ed together, so `after:X before:Y` is a range.
    pub date_filters: Vec<DateFilter>,
    /// Size filters, AND-ed together, so `size:>1mb size:<10mb` is a band.
    pub size_filters: Vec<SizeFilter>,
    /// Explicit attachment filter: `Some(true)` for has:attachment,
    /// `Some(false)` for has:no-attachment, `None` if unspecified.
    pub has_attachment: Option<bool>,
    /// Whether any term targets the Body or Filename field (requires
    /// full-text search).
    pub needs_fulltext: bool,
}

impl SearchQuery {
    /// Every term of every group, flattened.
    pub fn all_terms(&self) -> impl Iterator<Item = &SearchTerm> {
        self.groups.iter().flat_map(|g| g.terms.iter())
    }

    /// Whether the query carries no terms and no filters.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
            && self.date_filters.is_empty()
            && self.size_filters.is_empty()
            && self.has_attachment.is_none()
    }
}

/// Parse a query string into a structured [`SearchQuery`].
///
/// Never fails — unrecognized syntax is treated as a plain text search.
pub fn parse_query(input: &str) -> SearchQuery {
    let input = input.trim();

    let mut groups: Vec<TermGroup> = Vec::new();
    let mut date_filters = Vec::new();
    let mut size_filters = Vec::new();
    let mut has_attachment = None;
    let mut needs_fulltext = false;
    // Set by an `OR` token: the next term joins the group before it instead of
    // opening one of its own.
    let mut join_previous = false;

    let tokens = tokenize(input);

    // Collect a term into the current group — the previous one right after an
    // `OR`, a fresh one otherwise.
    macro_rules! push_term {
        ($term:expr) => {{
            let term = $term;
            match groups.last_mut() {
                Some(group) if join_previous => group.terms.push(term),
                _ => groups.push(TermGroup { terms: vec![term] }),
            }
        }};
    }

    for token in &tokens {
        if token == "OR" {
            // A dangling `OR` — leading, trailing, or next to a filter rather
            // than a term — has nothing to join and is ignored.
            join_previous = true;
            continue;
        }

        let (negated, token) = if let Some(stripped) = token.strip_prefix('-') {
            (true, stripped)
        } else {
            (false, token.as_str())
        };

        // Field:value pairs
        if let Some(value) = token.strip_prefix("from:") {
            push_term!(SearchTerm {
                field: SearchField::From,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("to:") {
            push_term!(SearchTerm {
                field: SearchField::To,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("cc:") {
            push_term!(SearchTerm {
                field: SearchField::Cc,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("subject:") {
            push_term!(SearchTerm {
                field: SearchField::Subject,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("body:") {
            needs_fulltext = true;
            push_term!(SearchTerm {
                field: SearchField::Body,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("label:") {
            push_term!(SearchTerm {
                field: SearchField::Label,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("filename:") {
            needs_fulltext = true;
            push_term!(SearchTerm {
                field: SearchField::Filename,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("id:") {
            push_term!(SearchTerm {
                field: SearchField::MessageId,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("has:") {
            match value {
                "attachment" | "attachments" => has_attachment = Some(!negated),
                "no-attachment" | "no-attachments" => has_attachment = Some(negated),
                _ => {}
            }
        } else if let Some(value) = token.strip_prefix("date:") {
            // Filters accumulate instead of replacing each other, so
            // `after:X before:Y` is a range rather than just whichever came
            // last, and a nonsensical combination returns nothing rather than
            // silently dropping half of what was typed.
            date_filters.extend(parse_date_filter(value));
        } else if let Some(value) = token.strip_prefix("before:") {
            date_filters.extend(parse_naive_date(value).map(DateFilter::Before));
        } else if let Some(value) = token.strip_prefix("after:") {
            date_filters.extend(parse_naive_date(value).map(DateFilter::After));
        } else if let Some(value) = token.strip_prefix("size:") {
            size_filters.extend(parse_size_filter(value));
        } else {
            // Plain text — search All fields
            push_term!(SearchTerm {
                field: SearchField::All,
                operator: make_operator(token),
                negated,
            });
        }

        join_previous = false;
    }

    SearchQuery {
        groups,
        date_filters,
        size_filters,
        has_attachment,
        needs_fulltext,
    }
}

/// Build an operator from a value string (quoted → Exact, otherwise → Contains).
fn make_operator(value: &str) -> SearchOperator {
    let unquoted = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value);
    if value.starts_with('"') && value.ends_with('"') {
        SearchOperator::Exact(unquoted.to_lowercase())
    } else {
        SearchOperator::Contains(unquoted.to_lowercase())
    }
}

/// Tokenize input respecting quoted strings.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parse a date filter value like `2024-01-01`, `2024-01`, `2024`,
/// or a range `2024-01-01..2024-06-30` (partial dates accepted in ranges).
fn parse_date_filter(value: &str) -> Option<DateFilter> {
    if let Some((start, end)) = value.split_once("..") {
        let s = parse_flexible_date_start(start)?;
        let e = parse_flexible_date_end(end)?;
        return Some(DateFilter::Range(s, e));
    }

    // Try full date
    if let Some(d) = parse_naive_date(value) {
        return Some(DateFilter::Exact(d));
    }

    // Try year-month: 2024-01
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() == 2 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        if (1..=12).contains(&month) {
            return Some(DateFilter::Month(year, month));
        }
    }

    // Try year only: 2024
    if parts.len() == 1 {
        let year: i32 = parts[0].parse().ok()?;
        if (1970..=2100).contains(&year) {
            return Some(DateFilter::Year(year));
        }
    }

    None
}

/// Parse a date string like `2024-01-04`.
fn parse_naive_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Parse a flexible date, returning the first day of the period.
/// Accepts `YYYY-MM-DD`, `YYYY-MM` (→ first of month), `YYYY` (→ Jan 1).
fn parse_flexible_date_start(s: &str) -> Option<NaiveDate> {
    if let Some(d) = parse_naive_date(s) {
        return Some(d);
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() == 2 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, 1);
    }
    if parts.len() == 1 {
        let year: i32 = parts[0].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, 1, 1);
    }
    None
}

/// Parse a flexible date, returning the last day of the period.
/// Accepts `YYYY-MM-DD`, `YYYY-MM` (→ last of month), `YYYY` (→ Dec 31).
fn parse_flexible_date_end(s: &str) -> Option<NaiveDate> {
    if let Some(d) = parse_naive_date(s) {
        return Some(d);
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() == 2 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        // Last day of month: go to first of next month, subtract 1 day
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let first_of_next = NaiveDate::from_ymd_opt(ny, nm, 1)?;
        return first_of_next.pred_opt();
    }
    if parts.len() == 1 {
        let year: i32 = parts[0].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, 12, 31);
    }
    None
}

/// Parse a size filter like `>1mb` or `<100kb`.
fn parse_size_filter(value: &str) -> Option<SizeFilter> {
    let (cmp, rest) = match value.strip_prefix('>') {
        Some(r) => (true, r),
        // Anything that isn't `>` or `<` is not a size filter at all.
        None => (false, value.strip_prefix('<')?),
    };

    let rest_lower = rest.to_lowercase();
    let (num_str, multiplier) = if let Some(n) = rest_lower.strip_suffix("gb") {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = rest_lower.strip_suffix("mb") {
        (n, 1024 * 1024)
    } else if let Some(n) = rest_lower.strip_suffix("kb") {
        (n, 1024)
    } else if let Some(n) = rest_lower.strip_suffix('b') {
        (n, 1u64)
    } else {
        (rest_lower.as_str(), 1u64)
    };

    let num: u64 = num_str.parse().ok()?;
    let bytes = num.checked_mul(multiplier)?;

    if cmp {
        Some(SizeFilter::GreaterThan(bytes))
    } else {
        Some(SizeFilter::LessThan(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every term of the query, flattened — most assertions here predate
    /// grouping and only care about what was parsed, not how it was grouped.
    fn terms(q: &SearchQuery) -> Vec<&SearchTerm> {
        q.all_terms().collect()
    }

    /// The single date filter of a query that has exactly one.
    fn only_date(q: &SearchQuery) -> &DateFilter {
        assert_eq!(q.date_filters.len(), 1, "expected exactly one date filter");
        &q.date_filters[0]
    }

    /// The single size filter of a query that has exactly one.
    fn only_size(q: &SearchQuery) -> &SizeFilter {
        assert_eq!(q.size_filters.len(), 1, "expected exactly one size filter");
        &q.size_filters[0]
    }

    #[test]
    fn test_parse_simple_query() {
        let q = parse_query("hello");
        assert_eq!(terms(&q).len(), 1);
        assert_eq!(terms(&q)[0].field, SearchField::All);
        assert!(!terms(&q)[0].negated);
        assert!(!q.needs_fulltext);
    }

    #[test]
    fn test_parse_multiword_free_text() {
        // The Search Filters "Text" field emits its value verbatim, so a
        // multi-word value tokenizes into one free-text term per word, ANDed.
        let q = parse_query("multi word search");
        assert_eq!(terms(&q).len(), 3);
        assert!(terms(&q).iter().all(|t| t.field == SearchField::All));
        assert_eq!(q.groups.len(), 3, "no OR, so each term is its own group");
    }

    #[test]
    fn test_parse_field_query() {
        let q = parse_query("from:user@example.com subject:hello");
        assert_eq!(terms(&q).len(), 2);
        assert_eq!(terms(&q)[0].field, SearchField::From);
        assert_eq!(terms(&q)[1].field, SearchField::Subject);
    }

    #[test]
    fn test_parse_negation() {
        let q = parse_query("-subject:spam");
        assert_eq!(terms(&q).len(), 1);
        assert!(terms(&q)[0].negated);
        assert_eq!(terms(&q)[0].field, SearchField::Subject);
    }

    #[test]
    fn test_parse_has_attachment() {
        let q = parse_query("has:attachment");
        assert_eq!(q.has_attachment, Some(true));
        assert!(terms(&q).is_empty());
    }

    #[test]
    fn test_parse_has_no_attachment() {
        let q = parse_query("has:no-attachment");
        assert_eq!(q.has_attachment, Some(false));
    }

    #[test]
    fn test_parse_date_exact() {
        let q = parse_query("date:2024-01-15");
        if let DateFilter::Exact(d) = only_date(&q) {
            assert_eq!(d.to_string(), "2024-01-15");
        } else {
            panic!("expected Exact date filter");
        }
    }

    #[test]
    fn test_parse_date_range() {
        let q = parse_query("date:2024-01-01..2024-06-30");
        if let DateFilter::Range(s, e) = only_date(&q) {
            assert_eq!(s.to_string(), "2024-01-01");
            assert_eq!(e.to_string(), "2024-06-30");
        } else {
            panic!("expected Range date filter");
        }
    }

    #[test]
    fn test_parse_date_month() {
        let q = parse_query("date:2024-01");
        if let DateFilter::Month(y, m) = only_date(&q) {
            assert_eq!(*y, 2024);
            assert_eq!(*m, 1);
        } else {
            panic!("expected Month date filter");
        }
    }

    #[test]
    fn test_parse_date_year() {
        let q = parse_query("date:2024");
        if let DateFilter::Year(y) = only_date(&q) {
            assert_eq!(*y, 2024);
        } else {
            panic!("expected Year date filter");
        }
    }

    #[test]
    fn test_parse_before_after() {
        assert!(matches!(
            only_date(&parse_query("before:2024-06-01")),
            DateFilter::Before(_)
        ));
        assert!(matches!(
            only_date(&parse_query("after:2024-01-01")),
            DateFilter::After(_)
        ));
    }

    #[test]
    fn test_date_filters_accumulate() {
        // `after:` and `before:` together describe a range. Only the last one
        // used to survive, so half of what the user typed was dropped.
        let q = parse_query("after:2024-01-01 before:2025-01-01");
        assert_eq!(q.date_filters.len(), 2);
        assert!(matches!(q.date_filters[0], DateFilter::After(_)));
        assert!(matches!(q.date_filters[1], DateFilter::Before(_)));
    }

    #[test]
    fn test_size_filters_accumulate() {
        let q = parse_query("size:>1mb size:<10mb");
        assert_eq!(q.size_filters.len(), 2);
    }

    #[test]
    fn test_parse_size_filter() {
        let q = parse_query("size:>1mb");
        if let SizeFilter::GreaterThan(b) = only_size(&q) {
            assert_eq!(*b, 1024 * 1024);
        } else {
            panic!("expected GreaterThan size filter");
        }

        let q = parse_query("size:<100kb");
        if let SizeFilter::LessThan(b) = only_size(&q) {
            assert_eq!(*b, 100 * 1024);
        } else {
            panic!("expected LessThan size filter");
        }

        // Bare-byte suffix.
        let q = parse_query("size:>500b");
        if let SizeFilter::GreaterThan(b) = only_size(&q) {
            assert_eq!(*b, 500);
        } else {
            panic!("expected GreaterThan size filter");
        }
    }

    #[test]
    fn test_parse_size_filter_overflow_is_ignored() {
        // A value that overflows u64 when multiplied by the unit must yield no
        // size filter instead of panicking (debug) or wrapping (release).
        let q = parse_query("size:>99999999999gb");
        assert!(q.size_filters.is_empty());
    }

    #[test]
    fn test_parse_body_triggers_fulltext() {
        let q = parse_query("body:important");
        assert!(q.needs_fulltext);
        assert_eq!(terms(&q)[0].field, SearchField::Body);
    }

    #[test]
    fn test_parse_or_query() {
        let q = parse_query("from:alice OR from:bob");
        assert_eq!(q.groups.len(), 1, "OR keeps both terms in one group");
        assert_eq!(q.groups[0].terms.len(), 2);
    }

    #[test]
    fn test_or_binds_tighter_than_implicit_and() {
        // `(from:alice OR from:bob) AND subject:invoice` — the whole query used
        // to become an OR, so anything with a matching subject came back too.
        let q = parse_query("from:alice OR from:bob subject:invoice");
        assert_eq!(q.groups.len(), 2);
        assert_eq!(q.groups[0].terms.len(), 2);
        assert_eq!(q.groups[0].terms[0].field, SearchField::From);
        assert_eq!(q.groups[0].terms[1].field, SearchField::From);
        assert_eq!(q.groups[1].terms.len(), 1);
        assert_eq!(q.groups[1].terms[0].field, SearchField::Subject);
    }

    #[test]
    fn test_or_chain_stays_in_one_group() {
        let q = parse_query("a OR b OR c");
        assert_eq!(q.groups.len(), 1);
        assert_eq!(q.groups[0].terms.len(), 3);
    }

    #[test]
    fn test_and_only_query_is_one_group_per_term() {
        let q = parse_query("from:alice subject:invoice");
        assert_eq!(q.groups.len(), 2);
        assert!(q.groups.iter().all(|g| g.terms.len() == 1));
    }

    #[test]
    fn test_dangling_or_is_ignored() {
        // Leading, trailing and doubled `OR` have nothing to join.
        for input in [
            "OR from:alice",
            "from:alice OR",
            "from:alice OR OR from:bob",
        ] {
            let q = parse_query(input);
            assert!(!q.groups.is_empty(), "{input} parsed to nothing");
            assert!(
                q.all_terms().all(|t| t.field == SearchField::From),
                "{input} produced a stray term"
            );
        }
        // The doubled OR still joins the two it sits between.
        assert_eq!(parse_query("from:alice OR OR from:bob").groups.len(), 1);
    }

    #[test]
    fn test_or_next_to_a_filter_falls_back_to_and() {
        // `OR` joins terms, not filters: the date filter stays an AND
        // condition and the term next to it opens its own group.
        let q = parse_query("date:2024 OR from:alice");
        assert_eq!(q.date_filters.len(), 1);
        assert_eq!(q.groups.len(), 1);
        assert_eq!(q.groups[0].terms.len(), 1);
    }

    #[test]
    fn test_group_needs_body() {
        assert!(parse_query("body:hello").groups[0].needs_body());
        assert!(parse_query("filename:report.pdf").groups[0].needs_body());
        // Free-text searches metadata *or* body, so it needs the body too.
        assert!(parse_query("hello").groups[0].needs_body());
        assert!(!parse_query("subject:hello").groups[0].needs_body());
        // A mixed group needs the body because one of its terms does.
        assert!(parse_query("subject:hello OR body:hello").groups[0].needs_body());
    }
    #[test]
    fn test_parse_quoted_phrase() {
        let q = parse_query("subject:\"hello world\"");
        assert_eq!(terms(&q).len(), 1);
        if let SearchOperator::Exact(ref s) = terms(&q)[0].operator {
            assert_eq!(s, "hello world");
        } else {
            panic!("expected Exact operator");
        }
    }

    #[test]
    fn test_parse_combined_query() {
        let q = parse_query("from:user1 subject:budget date:2024-01..2024-06 has:attachment");
        assert_eq!(terms(&q).len(), 2);
        assert_eq!(terms(&q)[0].field, SearchField::From);
        assert_eq!(terms(&q)[1].field, SearchField::Subject);
        assert_eq!(q.date_filters.len(), 1);
        assert_eq!(q.has_attachment, Some(true));
    }

    #[test]
    fn test_parse_empty_query() {
        let q = parse_query("");
        assert!(terms(&q).is_empty());
        assert!(q.date_filters.is_empty());
        assert!(q.size_filters.is_empty());
        assert!(q.has_attachment.is_none());
    }
}
