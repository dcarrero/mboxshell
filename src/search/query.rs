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
    /// Before a date (exclusive).
    Before(NaiveDate),
    /// After a date (exclusive).
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

/// A fully parsed search query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Text-based search terms (AND by default).
    pub terms: Vec<SearchTerm>,
    /// Optional date filter.
    pub date_filter: Option<DateFilter>,
    /// Optional size filter.
    pub size_filter: Option<SizeFilter>,
    /// Explicit attachment filter: `Some(true)` for has:attachment,
    /// `Some(false)` for has:no-attachment, `None` if unspecified.
    pub has_attachment: Option<bool>,
    /// Whether any term targets the Body field (requires full-text search).
    pub needs_fulltext: bool,
    /// Whether this is an OR query (any term matches) vs AND (all must match).
    pub is_or: bool,
}

/// Parse a query string into a structured [`SearchQuery`].
///
/// Never fails — unrecognized syntax is treated as a plain text search.
pub fn parse_query(input: &str) -> SearchQuery {
    let input = input.trim();

    let mut terms = Vec::new();
    let mut date_filter = None;
    let mut size_filter = None;
    let mut has_attachment = None;
    let mut needs_fulltext = false;
    let mut is_or = false;

    let tokens = tokenize(input);

    // Check for OR between tokens
    let has_or = tokens.iter().any(|t| t == "OR");
    if has_or {
        is_or = true;
    }

    for token in &tokens {
        if token == "OR" {
            continue;
        }

        let (negated, token) = if let Some(stripped) = token.strip_prefix('-') {
            (true, stripped)
        } else {
            (false, token.as_str())
        };

        // Field:value pairs
        if let Some(value) = token.strip_prefix("from:") {
            terms.push(SearchTerm {
                field: SearchField::From,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("to:") {
            terms.push(SearchTerm {
                field: SearchField::To,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("cc:") {
            terms.push(SearchTerm {
                field: SearchField::Cc,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("subject:") {
            terms.push(SearchTerm {
                field: SearchField::Subject,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("body:") {
            needs_fulltext = true;
            terms.push(SearchTerm {
                field: SearchField::Body,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("label:") {
            terms.push(SearchTerm {
                field: SearchField::Label,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("filename:") {
            needs_fulltext = true;
            terms.push(SearchTerm {
                field: SearchField::Filename,
                operator: make_operator(value),
                negated,
            });
        } else if let Some(value) = token.strip_prefix("id:") {
            terms.push(SearchTerm {
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
            date_filter = parse_date_filter(value);
        } else if let Some(value) = token.strip_prefix("before:") {
            if let Some(d) = parse_naive_date(value) {
                date_filter = Some(DateFilter::Before(d));
            }
        } else if let Some(value) = token.strip_prefix("after:") {
            if let Some(d) = parse_naive_date(value) {
                date_filter = Some(DateFilter::After(d));
            }
        } else if let Some(value) = token.strip_prefix("size:") {
            size_filter = parse_size_filter(value);
        } else {
            // Plain text — search All fields
            terms.push(SearchTerm {
                field: SearchField::All,
                operator: make_operator(token),
                negated,
            });
        }
    }

    SearchQuery {
        terms,
        date_filter,
        size_filter,
        has_attachment,
        needs_fulltext,
        is_or,
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
    let (cmp, rest) = if let Some(r) = value.strip_prefix('>') {
        (true, r)
    } else if let Some(r) = value.strip_prefix('<') {
        (false, r)
    } else {
        return None;
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
    let bytes = num * multiplier;

    if cmp {
        Some(SizeFilter::GreaterThan(bytes))
    } else {
        Some(SizeFilter::LessThan(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_query() {
        let q = parse_query("hello");
        assert_eq!(q.terms.len(), 1);
        assert_eq!(q.terms[0].field, SearchField::All);
        assert!(!q.terms[0].negated);
        assert!(!q.needs_fulltext);
    }

    #[test]
    fn test_parse_field_query() {
        let q = parse_query("from:user@example.com subject:hello");
        assert_eq!(q.terms.len(), 2);
        assert_eq!(q.terms[0].field, SearchField::From);
        assert_eq!(q.terms[1].field, SearchField::Subject);
    }

    #[test]
    fn test_parse_negation() {
        let q = parse_query("-subject:spam");
        assert_eq!(q.terms.len(), 1);
        assert!(q.terms[0].negated);
        assert_eq!(q.terms[0].field, SearchField::Subject);
    }

    #[test]
    fn test_parse_has_attachment() {
        let q = parse_query("has:attachment");
        assert_eq!(q.has_attachment, Some(true));
        assert!(q.terms.is_empty());
    }

    #[test]
    fn test_parse_has_no_attachment() {
        let q = parse_query("has:no-attachment");
        assert_eq!(q.has_attachment, Some(false));
    }

    #[test]
    fn test_parse_date_exact() {
        let q = parse_query("date:2024-01-15");
        assert!(q.date_filter.is_some());
        if let Some(DateFilter::Exact(d)) = &q.date_filter {
            assert_eq!(d.to_string(), "2024-01-15");
        } else {
            panic!("expected Exact date filter");
        }
    }

    #[test]
    fn test_parse_date_range() {
        let q = parse_query("date:2024-01-01..2024-06-30");
        if let Some(DateFilter::Range(s, e)) = &q.date_filter {
            assert_eq!(s.to_string(), "2024-01-01");
            assert_eq!(e.to_string(), "2024-06-30");
        } else {
            panic!("expected Range date filter");
        }
    }

    #[test]
    fn test_parse_date_month() {
        let q = parse_query("date:2024-01");
        if let Some(DateFilter::Month(y, m)) = &q.date_filter {
            assert_eq!(*y, 2024);
            assert_eq!(*m, 1);
        } else {
            panic!("expected Month date filter, got {:?}", q.date_filter);
        }
    }

    #[test]
    fn test_parse_date_year() {
        let q = parse_query("date:2024");
        if let Some(DateFilter::Year(y)) = &q.date_filter {
            assert_eq!(*y, 2024);
        } else {
            panic!("expected Year date filter");
        }
    }

    #[test]
    fn test_parse_before_after() {
        let q = parse_query("before:2024-06-01");
        assert!(matches!(q.date_filter, Some(DateFilter::Before(_))));

        let q = parse_query("after:2024-01-01");
        assert!(matches!(q.date_filter, Some(DateFilter::After(_))));
    }

    #[test]
    fn test_parse_size_filter() {
        let q = parse_query("size:>1mb");
        if let Some(SizeFilter::GreaterThan(b)) = &q.size_filter {
            assert_eq!(*b, 1024 * 1024);
        } else {
            panic!("expected GreaterThan size filter");
        }

        let q = parse_query("size:<100kb");
        if let Some(SizeFilter::LessThan(b)) = &q.size_filter {
            assert_eq!(*b, 100 * 1024);
        } else {
            panic!("expected LessThan size filter");
        }
    }

    #[test]
    fn test_parse_body_triggers_fulltext() {
        let q = parse_query("body:important");
        assert!(q.needs_fulltext);
        assert_eq!(q.terms[0].field, SearchField::Body);
    }

    #[test]
    fn test_parse_or_query() {
        let q = parse_query("from:alice OR from:bob");
        assert!(q.is_or);
        assert_eq!(q.terms.len(), 2);
    }

    #[test]
    fn test_parse_quoted_phrase() {
        let q = parse_query("subject:\"hello world\"");
        assert_eq!(q.terms.len(), 1);
        if let SearchOperator::Exact(ref s) = q.terms[0].operator {
            assert_eq!(s, "hello world");
        } else {
            panic!("expected Exact operator");
        }
    }

    #[test]
    fn test_parse_combined_query() {
        let q = parse_query("from:user1 subject:budget date:2024-01..2024-06 has:attachment");
        assert_eq!(q.terms.len(), 2);
        assert_eq!(q.terms[0].field, SearchField::From);
        assert_eq!(q.terms[1].field, SearchField::Subject);
        assert!(q.date_filter.is_some());
        assert_eq!(q.has_attachment, Some(true));
    }

    #[test]
    fn test_parse_empty_query() {
        let q = parse_query("");
        assert!(q.terms.is_empty());
        assert!(q.date_filter.is_none());
        assert!(q.size_filter.is_none());
        assert!(q.has_attachment.is_none());
    }
}
