//! Low-level natural-language slot extraction: quantities with units, dates, money, buyer
//! names. These run before any model is consulted (PRD §8.2), which is what keeps the common
//! path deterministic, instant, and free of RAM cost.

use crate::util;
use chrono::{Datelike, Duration, NaiveDate};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static RE_QTY_UNIT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(\d+(?:\.\d+)?)\s*(box(?:es)?|packet(?:s)?|pack(?:s)?|strip(?:s)?|tablet(?:s)?|tab(?:s)?|capsule(?:s)?|bottle(?:s)?|vial(?:s)?|ampoule(?:s)?|amp(?:s)?|strip|kg|kilo(?:gram)?s?|g|gram(?:s)?|mg|l|litre(?:s)?|liter(?:s)?|ml|pcs?|pieces?|units?|nos?|nos|each|dozen)?",
    )
    .unwrap()
});

static RE_MONEY: Lazy<Regex> = Lazy::new(|| {
    // Single line on purpose: a `\` line-continuation inside a Rust *raw* string is a literal
    // backslash-plus-newline, not a continuation, and silently corrupts the alternation.
    Regex::new(
        r"(?i)(?:₹|\brs\.?\s*|\binr\.?\s*|\brupees?\b)\s*(\d+(?:\.\d{1,2})?)|(\d+(?:\.\d{1,2})?)\s*(?:/-|\brs\b|\brupees?\b|₹)|(?:\bat\b|\bprice[ds]?\b|\brate\b|@)\s*(?:₹|rs\.?\s*)?\s*(\d+(?:\.\d{1,2})?)",
    )
    .unwrap()
});

static RE_ISO_DATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap());

static RE_DMY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(\d{1,2})[/](\d{1,2})[/](\d{2,4})\b").unwrap());

static RE_WORD_DATE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(\d{1,2})(?:st|nd|rd|th)?\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?,?\s*(\d{4})?\b|\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+(\d{1,2})(?:st|nd|rd|th)?,?\s*(\d{4})?\b",
    )
    .unwrap()
});

static RE_REL_DAYS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:in\s+)?\+?\s*(\d{1,4})\s*(day|week|month|year|yr)s?\b").unwrap());

static RE_BUYER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:to|for|sold\s+to|buyer:?|customer:?|purchased\s+by)\s+([a-z][a-z.'\- ]{1,60})").unwrap()
});

/// Units whose noun differs from the product's stored unit but means the same thing.
const UNIT_SYNONYMS: &[(&str, &str)] = &[
    ("pcs", "pcs"),
    ("piece", "pcs"),
    ("pieces", "pcs"),
    ("pc", "pcs"),
    ("each", "pcs"),
    ("unit", "pcs"),
    ("units", "pcs"),
    ("no", "pcs"),
    ("nos", "pcs"),
    ("tablet", "tab"),
    ("tablets", "tab"),
    ("tab", "tab"),
    ("tabs", "tab"),
    ("capsule", "caps"),
    ("capsules", "caps"),
    ("cap", "caps"),
    ("caps", "caps"),
    ("box", "box"),
    ("boxes", "box"),
    ("packet", "pack"),
    ("packets", "pack"),
    ("pack", "pack"),
    ("packs", "pack"),
    ("strip", "strip"),
    ("strips", "strip"),
    ("bottle", "bottle"),
    ("bottles", "bottle"),
    ("vial", "vial"),
    ("vials", "vial"),
    ("kilo", "kg"),
    ("kilos", "kg"),
    ("kilogram", "kg"),
    ("kilograms", "kg"),
    ("gram", "g"),
    ("grams", "g"),
    ("mg", "mg"),
    ("litre", "l"),
    ("litres", "l"),
    ("liter", "l"),
    ("liters", "l"),
    ("l", "l"),
];

pub fn canonical_unit(u: &str) -> String {
    let k = u.trim().to_lowercase();
    UNIT_SYNONYMS
        .iter()
        .find(|(from, _)| *from == k)
        .map(|(_, to)| to.to_string())
        .unwrap_or(k)
}

fn word_number(w: &str) -> Option<f64> {
    let table: HashMap<&str, f64> = HashMap::from([
        ("a", 1.0),
        ("an", 1.0),
        ("one", 1.0),
        ("two", 2.0),
        ("three", 3.0),
        ("four", 4.0),
        ("five", 5.0),
        ("six", 6.0),
        ("seven", 7.0),
        ("eight", 8.0),
        ("nine", 9.0),
        ("ten", 10.0),
        ("eleven", 11.0),
        ("twelve", 12.0),
        ("thirteen", 13.0),
        ("fourteen", 14.0),
        ("fifteen", 15.0),
        ("sixteen", 16.0),
        ("seventeen", 17.0),
        ("eighteen", 18.0),
        ("nineteen", 19.0),
        ("twenty", 20.0),
        ("thirty", 30.0),
        ("forty", 40.0),
        ("fourty", 40.0),
        ("fifty", 50.0),
        ("sixty", 60.0),
        ("seventy", 70.0),
        ("eighty", 80.0),
        ("ninety", 90.0),
        ("hundred", 100.0),
    ]);
    table.get(w.trim().to_lowercase().as_str()).copied()
}

/// "quarter"/"half"/"three quarters of a dozen". Returns the unit count.
fn fraction_of_dozen(w: &str) -> Option<f64> {
    let n = match w.trim().to_lowercase().as_str() {
        "half" => 0.5,
        "quarter" | "a quarter" => 0.25,
        "third" | "a third" => 1.0 / 3.0,
        "three-quarters" | "three quarters" | "three-quarters'" => 0.75,
        _ => return None,
    };
    Some(n * 12.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub unit: Option<String>,
    /// True when the number came from a word ("forty") or a fraction ("half a dozen").
    pub approximate: bool,
    pub span: (usize, usize),
}

impl Quantity {
    pub fn as_count(&self) -> i64 {
        // Partial units round up: taking 2.4 kg of something is 3 kg off the shelf.
        self.value.ceil().max(1.0) as i64
    }
}

/// Extract the first quantity phrase. Handles "40", "40 units", "2.5 boxes", "3kg", "a dozen",
/// "half a kilo", "forty tablets".
pub fn parse_quantity(text: &str) -> Option<Quantity> {
    let lower = text.to_lowercase();
    let s = lower.trim();

    // "half a dozen" / "half a kilo"
    if let Some(pos) = s.find("half") {
        let tail = &s[pos + 4..];
        let tail_trimmed = tail.trim_start();
        if tail_trimmed.starts_with("dozen") || tail_trimmed.starts_with("a dozen") {
            return Some(Quantity {
                value: 6.0,
                unit: Some("dozen".into()),
                approximate: true,
                span: (pos, pos + 4 + tail.len()),
            });
        }
        let amount = word_number(tail_trimmed.strip_prefix('a').unwrap_or(tail_trimmed).trim())
            .or_else(|| tail_trimmed.split_whitespace().next().and_then(word_number))?;
        return Some(Quantity {
            value: amount / 2.0,
            unit: None,
            approximate: true,
            span: (pos, pos + 4 + tail.len()),
        });
    }

    // "a dozen" / "two dozen" / "quarter dozen"
    if let Some(m) = Regex::new(r"(?i)\b([a-z]+|\d+(?:\.\d+)?)\s+dozen\b")
        .unwrap()
        .captures(s)
    {
        // An unrecognised word before "dozen" must NOT default to 1: "quarter dozen" was
        // silently becoming a whole dozen, i.e. a 4x over-count on the stock movement.
        let n = match word_number(&m[1]).or_else(|| m[1].parse::<f64>().ok()) {
            Some(n) => n,
            None => match fraction_of_dozen(&m[1]) {
                Some(v) => return Some(Quantity {
                    value: v,
                    unit: Some("dozen".into()),
                    approximate: true,
                    span: span(&m),
                }),
                // Not a number and not a fraction: "some dozen" is not a quantity.
                None => return None,
            },
        };
        return Some(Quantity {
            value: n * 12.0,
            unit: Some("dozen".into()),
            approximate: false,
            span: span(&m),
        });
    }

    // Numeric with optional unit noun. Iterate every candidate because a digit run glued to an
// identifier ("batch B2401") must be skipped, not treated as the answer.
    for c in RE_QTY_UNIT.captures_iter(s) {
        let raw = &c[1];
        let m = match c.get(0) {
            Some(m) => m,
            None => continue,
        };
        // Reject digit runs that are part of a larger token: an identifier ("batch B2401"),
        // a year ("2027"), or part of a date ("2027-03-01", "03-01"). A real quantity is a
        // standalone number.
        let prev_bad = s[..m.start()]
            .chars()
            .next_back()
            .map(|ch| ch.is_alphanumeric() || ch == '-' || ch == '/' || ch == '.')
            .unwrap_or(false);
        let after = &s[m.end()..];
        let next_bad = after
            .chars()
            .next()
            .map(|ch| ch == '/' || (ch == '-' && after[1..].starts_with(char::is_numeric)))
            .unwrap_or(false);
        if prev_bad || next_bad || (c.get(2).is_none() && raw.len() >= 4) {
            continue;
        }
        let Ok(value) = raw.parse::<f64>() else { continue };
        if value <= 0.0 || value > 1_000_000.0 {
            continue;
        }
        return Some(Quantity {
            value,
            unit: c.get(2).map(|u| canonical_unit(u.as_str())),
            approximate: false,
            span: span(&c),
        });
    }

    // Word numbers: "forty tablets", "twelve"
    if let Some(c) = Regex::new(r"(?i)\b(a|an|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fourty|fifty|sixty|seventy|eighty|ninety|hundred)\s+([a-z]+)\b")
        .unwrap()
        .captures(s)
    {
        if let Some(v) = word_number(&c[1]) {
            let unit = canonical_unit(&c[2]);
            // Only treat the trailing noun as a unit if it is one we understand, otherwise the
            // word is probably a product name fragment ("five star").
            let known = UNIT_SYNONYMS.iter().any(|(f, _)| *f == unit);
            if known {
                return Some(Quantity {
                    value: v,
                    unit: Some(unit),
                    approximate: false,
                    span: span(&c),
                });
            }
        }
    }

    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct Money {
    pub amount: f64,
    pub span: (usize, usize),
}

/// Extract a price. Handles `₹12.50`, `rs 40`, `40/-`, `inr 12.5`, `12.5 rupees`,
/// `forty rupees`, and bare numbers directly followed by a currency marker.
pub fn parse_money(text: &str) -> Option<Money> {
    let lower = text.to_lowercase();
    if let Some(c) = RE_MONEY.captures(&lower) {
        let raw = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3))?;
        if let Ok(amount) = raw.as_str().parse::<f64>() {
            if amount.is_finite() && amount >= 0.0 && amount < 1e9 {
                return Some(Money {
                    amount,
                    span: span(&c),
                });
            }
        }
    }
    // "forty rupees"
    if let Some(c) = Regex::new(r"(?i)\b(a|an|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)\s+rupees?\b")
        .unwrap()
        .captures(&lower)
    {
        if let Some(v) = word_number(&c[1]) {
            return Some(Money {
                amount: v,
                span: span(&c),
            });
        }
    }
    None
}

const MONTHS: &[(&str, u32)] = &[
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];

/// Byte span of a whole capture. `regex::Captures` has no `start`/`end` of its own — those live
/// on the inner `Match` — so this keeps the call sites readable.
/// The byte range the match actually covers, with trailing whitespace trimmed. The quantity
/// pattern has a `\s*` before its optional unit group, so an unmatched unit leaves the span
/// ending in a space — harmless for stripping, but it makes a span check read `"40 "` and hide
/// genuine off-by-one errors.
fn span(c: &regex::Captures<'_>) -> (usize, usize) {
    let Some(m) = c.get(0) else { return (0, 0) };
    let mut end = m.end();
    while end > m.start() {
        let prev = m.as_str()[..end - m.start()].chars().next_back();
        match prev {
            Some(ch) if ch.is_whitespace() => end -= ch.len_utf8(),
            _ => break,
        }
    }
    (m.start(), end)
}

fn month_num(s: &str) -> Option<u32> {
    let k = s.to_lowercase();
    MONTHS
        .iter()
        .find(|(m, _)| *m == k || (k.len() > 3 && k.starts_with(m)))
        .map(|(_, n)| *n)
}

/// Extract an expiry-style date. Handles ISO, `1/3/2027`, `1 March 2027`, `March 1 2027`,
/// `+90 days`, `in 6 months`, `next month`, `tomorrow`.
pub fn parse_date_phrase(text: &str) -> Option<String> {
    parse_date_phrase_full(text).map(|(d, _)| d)
}

/// Same as [`parse_date_phrase`] but also returns the byte span the phrase occupied, so the
/// caller can blank it out when isolating the product name.
pub fn parse_date_phrase_full(text: &str) -> Option<(String, (usize, usize))> {
    let lower = text.to_lowercase();
    let today = util::today();

    if let Some(c) = RE_ISO_DATE.captures(&lower) {
        let y: i32 = c[1].parse().ok()?;
        let m: u32 = c[2].parse().ok()?;
        let d: u32 = c[3].parse().ok()?;
        return NaiveDate::from_ymd_opt(y, m, d)
            .map(|v| (v.format("%Y-%m-%d").to_string(), span(&c)));
    }

    if let Some(c) = RE_DMY.captures(&lower) {
        let a: u32 = c[1].parse().ok()?;
        let b: u32 = c[2].parse().ok()?;
        let mut y: i32 = c[3].parse().ok()?;
        if y < 100 {
            y += if y < 70 { 2000 } else { 1900 };
        }
        // Disambiguate only when one field cannot be a month; otherwise assume month-first,
        // which is the ISO-adjacent reading and avoids silently swapping a real date.
        let (day, mon) = if (1..=12).contains(&b) && !(1..=12).contains(&a) {
            (a, b)
        } else if (1..=12).contains(&a) && (1..=12).contains(&b) {
            (b, a)
        } else {
            (a, b)
        };
        return NaiveDate::from_ymd_opt(y, mon, day)
            .map(|v| (v.format("%Y-%m-%d").to_string(), span(&c)));
    }

    if let Some(c) = RE_WORD_DATE.captures(&lower) {
        // Branch A groups: 1=day 2=month 3=year.  Branch B groups: 4=month 5=day 6=year.
        let (day, mon, year) = if c.get(1).is_some() {
            let m = month_num(&c[2])?;
            let y = c
                .get(3)
                .and_then(|y| y.as_str().parse::<i32>().ok())
                .unwrap_or(today.year());
            (c.get(1)?.as_str().parse::<u32>().ok()?, m, y)
        } else {
            let m = month_num(&c[4])?;
            let y = c
                .get(6)
                .and_then(|y| y.as_str().parse::<i32>().ok())
                .unwrap_or(today.year());
            (c.get(5)?.as_str().parse::<u32>().ok()?, m, y)
        };
        return NaiveDate::from_ymd_opt(year, mon, day)
            .map(|v| (v.format("%Y-%m-%d").to_string(), span(&c)));
    }

    for (needle, offset) in [
        ("tomorrow", 0usize),
        ("next week", 0),
        ("next month", 0),
        ("next year", 0),
    ] {
        if let Some(p) = lower.find(needle) {
            let d = match needle {
                "tomorrow" => util::add_days(today, 1),
                "next week" => util::add_days(today, 7),
                "next month" => (today + Duration::days(31)).with_day(1)?.format("%Y-%m-%d").to_string(),
                _ => (today + Duration::days(365))
                    .with_month(1)?
                    .with_day(1)?
                    .format("%Y-%m-%d")
                    .to_string(),
            };
            return Some((d, (p + offset, p + needle.len())));
        }
    }

    if let Some(c) = RE_REL_DAYS.captures(&lower) {
        let n: i64 = c[1].parse().ok()?;
        let mult = match &c[2] {
            "day" => 1,
            "week" => 7,
            "month" => 30,
            _ => 365,
        };
        if !(0..=365 * 10).contains(&n) {
            return None;
        }
        return Some((util::add_days(today, n * mult), span(&c)));
    }

    None
}

/// Byte span of the date phrase, if any.
pub fn parse_date_phrase_span(text: &str) -> Option<(usize, usize)> {
    parse_date_phrase_full(text).map(|(_, span)| span)
}

/// `in 6 months` → 180, `+90 days` → 90, `2 weeks` → 14.
pub fn parse_relative_days(text: &str) -> Option<i64> {
    let lower = text.to_lowercase();
    let c = RE_REL_DAYS.captures(&lower)?;
    let n: i64 = c.get(1)?.as_str().parse().ok()?;
    let mult = match c.get(2)?.as_str() {
        "day" => 1,
        "week" => 7,
        "month" => 30,
        _ => 365,
    };
    if !(0..=365 * 10).contains(&n) {
        return None;
    }
    Some(n * mult)
}

/// Pull a name after `to` / `for` / `buyer:`. Strips honorifics and trailing punctuation but
/// keeps interior periods (`J. R. Sharma`) intact.
pub fn parse_buyer(text: &str) -> Option<String> {
    let c = RE_BUYER.captures(text)?;
    let raw = c.get(1)?.as_str();
    // Trim only *trailing* separators; splitting on '.' would chop "Mr. Sharma" to "Mr".
    let mut cleaned = raw.trim().trim_end_matches([',', '.', ';', ':']).trim();

    const HONORIFICS: &[&str] = &[
        "mr. ", "mr ", "mrs. ", "mrs ", "ms. ", "ms ", "dr. ", "dr ", "prof. ", "prof ",
        "shri. ", "shri ", "smt. ", "smt ",
    ];
    loop {
        let lowered = cleaned.to_lowercase();
        match HONORIFICS.iter().find(|h| lowered.starts_with(**h)) {
            Some(h) => cleaned = cleaned[h.len()..].trim(),
            None => break,
        }
    }
    // A trailing sentence period on an otherwise complete name is punctuation, not an initial.
    if cleaned.ends_with('.') && cleaned.matches('.').count() > 1 {
        cleaned = cleaned.trim_end_matches('.').trim();
    }

    if cleaned.len() < 2 || cleaned.len() > 60 {
        return None;
    }
    Some(cleaned.to_string())
}

/// Byte span of the buyer phrase, so it can be excluded when isolating the product name.
pub fn parse_buyer_span(text: &str) -> Option<(usize, usize)> {
    let c = RE_BUYER.captures(text)?;
    let name = parse_buyer(text)?;
    let start = c.get(0)?.start();
    let end = (c.get(1)?.end() + name.len() + 4).min(text.len());
    Some((start, end))
}

/// Remove every recognised span from `text`, leaving the words most likely to name a product.
pub fn strip_matched(text: &str, spans: &[(usize, usize)]) -> String {
    let mut ranges: Vec<(usize, usize)> = spans
        .iter()
        .copied()
        .filter(|(s, e)| s < e && *s < text.len() && *e <= text.len())
        .filter(|(s, e)| text.is_char_boundary(*s) && text.is_char_boundary(*e))
        .collect();
    ranges.sort_unstable();

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (start, end) in ranges {
        if start < cursor {
            continue;
        }
        out.push_str(&text[cursor..start]);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities() {
        assert_eq!(parse_quantity("add 40 units").unwrap().value, 40.0);
        assert_eq!(
            parse_quantity("2 boxes of amox").unwrap().unit.as_deref(),
            Some("box")
        );
        assert_eq!(parse_quantity("3kg").unwrap().value, 3.0);
        assert_eq!(parse_quantity("3kg").unwrap().unit.as_deref(), Some("kg"));
        assert_eq!(parse_quantity("a dozen").unwrap().value, 12.0);
        assert_eq!(parse_quantity("half a dozen").unwrap().value, 6.0);
        assert_eq!(parse_quantity("forty tablets").unwrap().value, 40.0);
        assert_eq!(
            parse_quantity("forty tablets").unwrap().unit.as_deref(),
            Some("tab")
        );
        // A bare year must never be read as a quantity.
        assert!(parse_quantity("expiry 2027-03-01").is_none());
        assert!(parse_quantity("show me the stock").is_none());
    }

    #[test]
    fn money() {
        assert_eq!(parse_money("at ₹12.50 each").unwrap().amount, 12.5);
        assert_eq!(parse_money("price rs 40").unwrap().amount, 40.0);
        assert_eq!(parse_money("40/-").unwrap().amount, 40.0);
        assert_eq!(parse_money("inr 12.5").unwrap().amount, 12.5);
        assert_eq!(parse_money("forty rupees").unwrap().amount, 40.0);
        assert!(parse_money("no price here").is_none());
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date_phrase("expiry 2027-03-01").unwrap(), "2027-03-01");
        assert_eq!(parse_date_phrase("expires 1 March 2027").unwrap(), "2027-03-01");
        assert_eq!(parse_date_phrase("March 1 2027").unwrap(), "2027-03-01");
        assert_eq!(parse_date_phrase("expires 1st March 2027").unwrap(), "2027-03-01");
        assert_eq!(parse_date_phrase("exp in 90 days").unwrap(), util::add_days(util::today(), 90));
        assert_eq!(parse_date_phrase("in 6 months").unwrap(), util::add_days(util::today(), 180));
        assert!(parse_date_phrase("no date").is_none());
    }

    #[test]
    fn buyers() {
        assert_eq!(parse_buyer("sold 5 Panadol to Mr. Sharma").unwrap(), "Sharma");
        assert_eq!(parse_buyer("sale to Priya Nair").unwrap(), "Priya Nair");
        assert!(parse_buyer("how much stock").is_none());
    }

    #[test]
    fn stripping() {
        let text = "add 40 amoxicillin batch B2401 expiry 2027-03-01 at 12.50";
        // Spans come from the real parsers, so this also proves the parsers agree on offsets.
        let mut spans = vec![
            parse_quantity(text).expect("quantity").span,
            parse_money(text).expect("money").span,
            parse_date_phrase_full(text).expect("date").1,
        ];
        spans.sort_unstable();
        let out = strip_matched(text, &spans);
        let tokens: Vec<&str> = out.split_whitespace().collect();
        assert!(out.contains("amoxicillin"), "{out}");
        assert!(out.contains("batch"), "{out}");
        // Assert on whole tokens: "B2401" legitimately still contains the substring "40".
        assert!(!tokens.contains(&"40"), "{out}");
        assert!(!tokens.contains(&"2027-03-01"), "{out}");
        assert!(!tokens.contains(&"12.50"), "{out}");
    }

    #[test]
    fn batch_numbers_are_not_quantities() {
        // "B1" must not be read as a quantity of 1.
        assert!(parse_quantity("add amoxicillin batch B1").is_none());
        assert_eq!(parse_quantity("batch B2401 for 40").unwrap().value, 40.0);
    }

    #[test]
    fn dates_reject_impossible_calendar_values() {
        // SQLite's date functions silently roll over or return NULL, which is how an expiry
        // becomes "1900-01-01" and a batch looks decades expired.
        for bad in ["2027-02-30", "2027-13-01", "2027-00-10", "31st February 2027"] {
            assert!(
                parse_date_phrase(bad).is_none(),
                "{bad:?} must be rejected rather than rolled over"
            );
        }
        // Month names out of range.
        assert!(parse_date_phrase("1 Smarch 2027").is_none());
    }

    #[test]
    fn leap_days_are_handled() {
        assert_eq!(parse_date_phrase("29 February 2028").unwrap(), "2028-02-29");
        // 2027 is not a leap year, so this must not become 2027-03-01.
        assert!(parse_date_phrase("29 February 2027").is_none());
    }

    #[test]
    fn money_rejects_nonsense() {
        // Negative money would invert a ledger total; a bare "40" with no currency marker is a
        // quantity, not a price.
        assert!(parse_money("-12.50").is_none());
        assert!(parse_money("price ₹-5").is_none());
        assert!(parse_money("received 40").is_none());
    }

    #[test]
    fn money_survives_every_currency_spelling() {
        // A `\` line-continuation inside a Rust raw string is a literal backslash-plus-newline, not
        // a continuation. That once corrupted the alternation so only one branch ever matched and
        // every currency format but "at N" returned None.
        for text in ["at 12.50", "at ₹12.50", "price rs 40", "rs. 40", "40/-", "inr 12.5", "Rs 40.25"] {
            assert!(parse_money(text).is_some(), "{text:?} should parse as money");
        }
    }

    #[test]
    fn buyers_keep_interior_initials_and_honorifics() {
        // Splitting on '.' and taking the first segment truncated "Mr. Sharma" to "Mr".
        assert_eq!(parse_buyer("to Mr. Sharma").unwrap(), "Sharma");
        assert_eq!(parse_buyer("to Dr. Anand Kumar").unwrap(), "Anand Kumar");
        // Interior initials are part of the name, not a truncation point.
        assert_eq!(parse_buyer("to J. R. Sharma").unwrap(), "J. R. Sharma");
        assert_eq!(parse_buyer("sold to Apollo Franchise Outlet").unwrap(), "Apollo Franchise Outlet");
        // Trailing punctuation is stripped, not interior.
        assert_eq!(parse_buyer("to Neha Gupta.").unwrap(), "Neha Gupta");
    }

    #[test]
    fn quantity_unit_vocabulary_is_consistent() {
        // Unit normalisation drives how a sale is counted, so the same unit must always
        // normalise to the same token.
        assert_eq!(parse_quantity("2 pieces").unwrap().unit.as_deref(), Some("pcs"));
        assert_eq!(parse_quantity("2 pcs").unwrap().unit.as_deref(), Some("pcs"));
        assert_eq!(parse_quantity("1 box").unwrap().unit.as_deref(), Some("box"));
        assert_eq!(parse_quantity("1 boxes").unwrap().unit.as_deref(), Some("box"));
        assert_eq!(parse_quantity("2.5 kg").unwrap().value, 2.5);
        assert_eq!(parse_quantity("1 dozen").unwrap().value, 12.0);
        assert_eq!(parse_quantity("quarter dozen").unwrap().value, 3.0);
    }

    #[test]
    fn spans_point_at_the_matched_text() {
        // Offsets are what `strip_matched` removes, so a span that does not cover its own value
        // silently leaks the value into the product reference.
        let text = "received 40 amox batch B2401 expiring 2027-03-01 at 12.50";
        let q = parse_quantity(text).expect("quantity");
        assert_eq!(&text[q.span.0..q.span.1], "40");

        let m = parse_money(text).expect("money");
        // The money span deliberately includes the "at"/"price" marker, so stripping it removes
        // the preposition from the product reference too. Assert the value is covered.
        assert!(
            text[m.span.0..m.span.1].contains("12.50"),
            "money span {:?} = {:?} must cover the amount",
            m.span,
            &text[m.span.0..m.span.1]
        );
        assert!(m.span.1 <= text.len(), "money span must stay inside the text");

        let (_, dspan) = parse_date_phrase_full(text).expect("date");
        assert_eq!(&text[dspan.0..dspan.1], "2027-03-01");

        let b = parse_buyer_span(text);
        assert!(b.is_none(), "no buyer in this sentence");
    }
}