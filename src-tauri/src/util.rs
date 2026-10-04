use chrono::{Datelike, NaiveDate};
use std::collections::HashSet;

/// Today in local time as `YYYY-MM-DD`. Expiry semantics follow the operator's calendar.
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

pub fn today_str() -> String {
    today().format("%Y-%m-%d").to_string()
}

pub fn now_ts() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parse a `YYYY-MM-DD` date, rejecting values SQLite would silently accept.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s.trim(), "%d/%m/%Y"))
        .or_else(|_| NaiveDate::parse_from_str(s.trim(), "%d-%m-%Y"))
        .ok()
}

/// Days from today until `date`. Negative when already past.
pub fn days_until(date: &str) -> i64 {
    match parse_date(date) {
        Some(d) => (d - today()).num_days(),
        None => i64::MAX / 4,
    }
}

pub fn days_since(date: &str) -> i64 {
    -days_until(date)
}

/// Days elapsed since `ts`, used by the dead-stock heuristic.
pub fn days_since_ts(ts: &str) -> i64 {
    match chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.fZ"))
    {
        Ok(dt) => (today() - dt.date()).num_days(),
        Err(_) => match parse_date(ts) {
            Some(d) => (today() - d).num_days(),
            None => 0,
        },
    }
}

pub fn add_days(d: NaiveDate, n: i64) -> String {
    (d + chrono::Duration::days(n)).format("%Y-%m-%d").to_string()
}

pub fn start_of_week(d: NaiveDate) -> String {
    let dow = d.weekday().num_days_from_monday() as i64;
    add_days(d, -dow)
}

fn split_alnum(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// FTS5 `porter unicode61` tokenizer terms, plus prefix variants for as-you-type search.
pub fn fts_terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for tok in split_alnum(query) {
        for variant in [tok.clone(), format!("{tok}*")] {
            if seen.insert(variant.clone()) {
                out.push(format!("'{}'", variant.replace('\'', "")));
            }
        }
    }
    out
}

/// Character-trigram set, used for `did-you-mean` when FTS returns nothing.
pub fn trigrams(s: &str) -> HashSet<String> {
    let norm: String = s
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    let chars: Vec<char> = norm.chars().collect();
    let mut set = HashSet::new();
    if chars.len() < 3 {
        if !norm.is_empty() {
            set.insert(norm.clone());
        }
        return set;
    }
    for w in chars.windows(3) {
        set.insert(w.iter().collect());
    }
    set
}

/// Sørensen–Dice coefficient over trigram sets. 0.0 – 1.0.
pub fn trigram_similarity(a: &str, b: &str) -> f64 {
    let ta = trigrams(a);
    let tb = trigrams(b);
    if ta.is_empty() || tb.is_empty() {
        return if a.eq_ignore_ascii_case(b) { 1.0 } else { 0.0 };
    }
    let inter = ta.intersection(&tb).count() as f64;
    2.0 * inter / (ta.len() as f64 + tb.len() as f64)
}

/// Collapse whitespace, trim, and return `None` when empty.
pub fn clean_opt(s: Option<&str>) -> Option<String> {
    s.map(|v| v.trim().replace(char::is_whitespace, " ").trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Truncate for LLM context without splitting a character.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}