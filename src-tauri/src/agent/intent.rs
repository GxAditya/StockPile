//! Deterministic intent parser (PRD §8.2). Runs **before** any model call. A rule-based parse
//! that is above confidence resolves straight to a verified tool call — zero RAM, zero latency,
//! always correct. Only genuinely ambiguous or analytical requests fall through to the model.

use super::extract;
use crate::db::repo;
use crate::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Restock,
    Sale,
    Adjust,
    QueryStock,
    QueryExpiring,
    QueryValue,
    QueryHistory,
    QueryBuyers,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedIntent {
    pub action: Action,
    pub product_query: Option<String>,
    pub quantity: Option<i64>,
    pub unit: Option<String>,
    pub batch_no: Option<String>,
    pub expiry: Option<String>,
    pub price: Option<f64>,
    pub buyer: Option<String>,
    pub within_days: Option<i64>,
    pub confidence: f64,
    /// Populated when the product reference was ambiguous.
    pub candidates: Vec<String>,
    /// Slot values we could not source, so the caller knows what to ask for.
    pub missing: Vec<&'static str>,
}

/// Verb lexicons, longest-match first so "sold" never shadows "sell".
const RESTOCK_VERBS: &[&str] = &[
    "received", "restocked", "stocked in", "stock in", "add stock", "added stock", "add",
    "restock", "receive", "inbound", "incoming", "got in", "arrived", "new stock", "put in",
    "load in", "put away", "intake",
    "bought", "buy", "purchased", "purchase", "procured", "obtained", "picked up",
];

const SALE_VERBS: &[&str] = &[
    "sold out", "sold", "sell", "dispatched", "dispatch", "shipped", "ship", "issued",
    "issue", "ordered by", "outbound", "delivered", "deliver",
];

const ADJUST_VERBS: &[&str] = &[
    "adjust", "correct", "correction", "damaged", "damage", "write off", "writeoff", "miscount",
    "reconcile", "shrink",
];

const QUERY_VERBS: &[&str] = &[
    "how many", "how much", "what's the", "whats the", "show me", "show all", "list all",
    "tell me", "check", "do i have", "do we have", "is there", "current stock", "stock of",
    "stock in", "inventory of", "lookup", "find", "search for", "get",
];

const EXPIRY_HINTS: &[&str] = &[
    "expiring", "expire", "expiry", "expiration", "about to expire", "shelf life", "perishing",
];

const VALUE_HINTS: &[&str] = &[
    "inventory value", "stock value", "total value", "worth", "valuation", "value of my",
    "how much value", "capital tied",
];

const HISTORY_HINTS: &[&str] = &[
    "history", "recent sales", "recent activity", "transactions", "what was sold", "log",
    "audit", "movement history", "recent movements",
];

const BUYER_HINTS: &[&str] = &["buyers", "customers", "who bought", "top buyers", "client"];

/// Words that make a request comparative rather than a fixed lookup. "Name the products with
/// the farthest expiry" cannot be answered by "everything expiring within 30 days" — the
/// deterministic window is the wrong question, so these must reach the model.
const COMPARATIVE_HINTS: &[&str] = &[
    "farthest", "furthest", "longest", "latest", "earliest", "closest", "soonest", "shortest",
    "most", "least", "best", "worst", "biggest", "smallest", "highest", "lowest", "maximum",
    "minimum", "compare", "versus", " vs ", "difference between", "which product", "which of",
    "rank", "top 5", "why", "because", "trend", "should i",
];

/// Substring search for a verb, but only on whole words. `find` alone made "buy" match inside
/// "buyers", which reclassified "who are my top buyers" as an inbound purchase — adding `buy`
/// to the restock lexicon silently broke every buyer question.
fn find_any(hay: &str, needles: &[&'static str]) -> Option<(usize, usize, &'static str)> {
    let mut best: Option<(usize, usize, &'static str)> = None;
    for &n in needles {
        let Some(p) = hay.find(n) else { continue };
        if !word_boundary(hay, p, p + n.len()) {
            continue;
        }
        let len = n.len();
        let better = match best {
            Some((bs, _, _)) => p < bs || (p == bs && len > hay[p..].len()),
            None => true,
        };
        if better {
            best = Some((p, p + len, n));
        }
    }
    best
}

/// True when `[start, end)` is not flanked by another alphanumeric character.
fn word_boundary(hay: &str, start: usize, end: usize) -> bool {
    let before_ok = hay[..start]
        .chars()
        .next_back()
        .map(|c| !c.is_alphanumeric())
        .unwrap_or(true);
    let after_ok = hay[end..]
        .chars()
        .next()
        .map(|c| !c.is_alphanumeric())
        .unwrap_or(true);
    before_ok && after_ok
}

fn find_labeled(text: &str, labels: &[&str]) -> Option<String> {
    let lower = text.to_lowercase();
    for label in labels {
        // `continue`, not `?`: a miss on "batch" must not abandon the whole search, otherwise
        // "lot 42" never resolves because "batch" is checked first.
        let Some(p) = lower.find(label) else { continue };
        let after = p + label.len();
        // "lot7" is a single token with no separator; taking only what follows the label would
        // capture "7" and lose the prefix. Treat a flush alphanumeric as part of the value.
        if lower[after..].starts_with(|c: char| c.is_alphanumeric()) {
            let end = text[after..]
                .find(char::is_whitespace)
                .map(|i| after + i)
                .unwrap_or(text.len());
            return Some(text[p..end].trim_matches(',').to_string());
        }
        let rest = text[after..].trim_start();
        let rest = rest.trim_start_matches([':', '=']).trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let candidate = rest[..end].trim().trim_matches(',').to_string();
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    None
}

/// A bare lot code: at least one digit and one letter, no whitespace, not a plain number.
/// Covers the common unlabelled forms — "amox b2401", "Panadol LOT7" — which operators write
/// constantly and which the label-based path misses entirely.
fn find_bare_batch(text: &str, skip: &[(usize, usize)]) -> Option<String> {
    for (off, word) in text.split_whitespace().enumerate() {
        let start: usize = text.split_whitespace().take(off).map(|w| w.len() + 1).sum();
        let start = start.min(text.len());
        let end = (start + word.len()).min(text.len());
        if skip.iter().any(|(s, e)| start < *e && end > *s) {
            continue; // overlaps the quantity, a date, or a money span
        }
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if clean.len() < 3 || clean.len() > 24 {
            continue;
        }
        let has_digit = clean.chars().any(|c| c.is_ascii_digit());
        let has_alpha = clean.chars().any(|c| c.is_ascii_alphabetic());
        if has_digit && has_alpha {
            return Some(clean.to_string());
        }
    }
    None
}

/// Conversational filler that is never part of a product name.
const GREETING: &[&str] = &[
    "hello", "hi", "hey", "hii", "yo", "please", "can", "you", "could", "would", "will",
    "tell", "let", "know", "want", "wanna", "need", "like", "help", "thanks", "thank",
    "hey", "good", "morning", "evening", "afternoon",
];

/// The deterministic parse. Pure text analysis; product resolution happens in `resolve`.
pub fn parse(text: &str) -> ParsedIntent {
    let original = text.trim();
    let lower = original.to_lowercase();

    let mut spans: Vec<(usize, usize)> = Vec::new();

    let qty = extract::parse_quantity(original);
    if let Some(q) = &qty {
        spans.push(q.span);
    }
    let money = extract::parse_money(original);
    if let Some(m) = &money {
        spans.push(m.span);
    }
    let date = extract::parse_date_phrase(original);
    if let Some(d) = &date {
        // Locate the date phrase to remove it; re-parse against the original to get its span.
        if let Some(m) = extract::parse_date_phrase_span(original) {
            let _ = d;
            spans.push(m);
        }
    }
    let buyer = extract::parse_buyer(original);
    if let Some(b) = &buyer {
        if let Some(span) = extract::parse_buyer_span(original) {
            let _ = b;
            spans.push(span);
        }
    }
    let batch_no = find_labeled(original, &["batch no", "batch#", "lot no", "batch", "lot"]);
    if let Some(b) = &batch_no {
        // Cover the label *and* its value so neither leaks into the product reference. The old
        // arithmetic (`p + 5 + b.len()`) assumed the value sat flush against the label, so
        // "lot 42" cut one character short and left a stray digit in the product query.
        let lower_b = b.to_lowercase();
        if let Some(p) = lower.find(&lower_b) {
            // Walk backwards over the label words immediately preceding the value, skipping the
            // separators between them. "Batch No : 23456" has a space and a colon in between, so
            // matching the label as one contiguous run failed and the stray "no" then leaked into
            // the product reference as "orymol no".
            let mut s = p;
            for label in ["batch no", "lot no", "batch#", "batch", "lot"] {
                let mut probe = s;
                // Skip trailing separators (space, colon, equals, dash, comma).
                while probe > 0 {
                    let ch = lower[..probe].chars().next_back().unwrap();
                    if ch.is_alphanumeric() {
                        break;
                    }
                    probe -= ch.len_utf8();
                }
                if probe >= label.len() {
                    let cand = probe - label.len();
                    if lower[cand..probe].trim() == label {
                        s = cand;
                        break;
                    }
                }
            }
            let bs = (s, (p + b.len()).min(original.len()));
            spans.push(bs);
        }
    }

    // ── Verb classification ─────────────────────────────────────────────────────
    let restock = find_any(&lower, RESTOCK_VERBS);
    let sale = find_any(&lower, SALE_VERBS);
    let adjust = find_any(&lower, ADJUST_VERBS);
    let query = find_any(&lower, QUERY_VERBS);

    let mut intent = ParsedIntent {
        action: Action::Unknown,
        product_query: None,
        quantity: qty.as_ref().map(|q| q.as_count()),
        unit: qty.as_ref().and_then(|q| q.unit.clone()),
        batch_no,
        expiry: date.clone(),
        price: money.as_ref().map(|m| m.amount),
        buyer,
        within_days: None,
        confidence: 0.0,
        candidates: Vec::new(),
        missing: Vec::new(),
    };

    // Explicit "in X days" on an expiry query narrows the window.
    if let Some(c) = extract::parse_relative_days(&lower) {
        intent.within_days = Some(c);
    }

    // Sale beats restock when both are plausible: "received and sold" is a sale phrase far more
    // often than an inbound one, and the explicit buyer name is a strong sale signal.
    let sale_score = sale.map(|(_, _, v)| v.len()).unwrap_or(0);
    let restock_score = restock.map(|(_, _, v)| v.len()).unwrap_or(0);
    let has_buyer = intent.buyer.is_some();
    let is_negative = lower.contains("didn't") || lower.contains("did not") || lower.contains("haven't")
        || lower.contains("dont") || lower.contains("don't");

    if let Some(span) = sale {
        intent.action = Action::Sale;
        intent.confidence = if has_buyer { 0.94 } else { 0.80 };
        if restock_score > sale_score && has_buyer {
            intent.confidence = 0.70;
        }
        spans.push((span.0, span.1));
    } else if let Some(span) = adjust {
        intent.action = Action::Adjust;
        intent.confidence = 0.82;
        spans.push((span.0, span.1));
    } else if let Some(span) = restock {
        intent.action = Action::Restock;
        intent.confidence = 0.76;
        // Restock needs a quantity to be actionable.
        if intent.quantity.is_none() {
            intent.confidence = 0.45;
            intent.missing.push("quantity");
        }
        spans.push((span.0, span.1));
    } else if let Some(span) = query {
        intent.action = Action::QueryStock;
        intent.confidence = 0.70;
        spans.push((span.0, span.1));
    }

    // Hints refine an already-classified or unclassified query.
    //
    // A mutation verb wins outright. "Bought Orymol V, Batch No 23456, Qty 450, Price Rs 40,
    // Expiry 02 Oct 2027" contains the word "expiry", but it is a *restock* — the expiry is a
    // slot being filled, not the subject of the question. Previously the hint overrode the verb
    // and the parser answered "1 batch expiring soon" instead of recording 450 units, silently
    // dropping the whole mutation. A sentence that already carries a write verb is never a query.
    let is_mutation = matches!(
        intent.action,
        Action::Restock | Action::Sale | Action::Adjust
    );
    if !is_mutation && (intent.action == Action::Unknown || intent.action == Action::QueryStock) {
        if EXPIRY_HINTS.iter().any(|h| lower.contains(h)) {
            intent.action = Action::QueryExpiring;
            intent.confidence = intent.confidence.max(0.80);
        } else if VALUE_HINTS.iter().any(|h| lower.contains(h)) {
            intent.action = Action::QueryValue;
            intent.confidence = intent.confidence.max(0.85);
        } else if HISTORY_HINTS.iter().any(|h| lower.contains(h)) {
            intent.action = Action::QueryHistory;
            intent.confidence = intent.confidence.max(0.85);
        } else if BUYER_HINTS.iter().any(|h| lower.contains(h)) {
            intent.action = Action::QueryBuyers;
            intent.confidence = intent.confidence.max(0.85);
        }
    }

    // A comparative request is not a fixed-window lookup. "Name the products with the farthest
    // expiry" was answered with "1 batch expiring within 30 days", which is a confident,
    // confidently-wrong answer. Hand these to the model instead of guessing a window.
    if !is_mutation && COMPARATIVE_HINTS.iter().any(|h| lower.contains(h)) {
        intent.confidence = intent.confidence.min(0.5);
    }

    // No batch was labelled, so look for a bare lot code among the surviving tokens. This has to
    // happen after the other spans are known so a date or money value is never mistaken for one,
    // and before the "missing" checks so a restock is not reported as unexecutable.
    if intent.batch_no.is_none() {
        let mut known: Vec<(usize, usize)> = spans.clone();
        if let Some((s, e, _)) = sale.or(restock).or(adjust).or(query) {
            known.push((s, e));
        }
        if let Some(bare) = find_bare_batch(&lower, &known) {
            // `bare` was matched against the lowercased copy, but batch numbers are
            // case-sensitive ("B2401" and "b2401" are different batches in the catalogue).
            // Slice the *original* at the same byte offsets to recover the real casing.
            if let Some(p) = lower.find(&bare) {
                let end = (p + bare.len()).min(original.len());
                spans.push((p, end));
                intent.batch_no = Some(original[p..end].to_string());
            }
        }
    }

    // A sale verb with no quantity is unexecutable; a query with no quantity is fine.
    if intent.action == Action::Sale && intent.quantity.is_none() {
        intent.missing.push("quantity");
        intent.confidence = intent.confidence.min(0.5);
    }
    if intent.action == Action::Sale && intent.buyer.is_none() {
        intent.missing.push("buyer");
    }
    if intent.action == Action::Restock && intent.batch_no.is_none() {
        intent.missing.push("batch");
    }

    // Product reference = whatever survives removing every recognised span. All offsets come
    // from the original text, so every span must be removed in a single pass — removing the
    // verb first would shift every later offset.
    let mut cut: Vec<(usize, usize)> = spans;
    if let Some((s, e, _)) = sale.or(restock).or(adjust).or(query) {
        cut.push((s, e));
    }
    let remainder = extract::strip_matched(&lower, &cut);

    const NOISE: &[&str] = &[
        "please", "can you", "could you", "i want to", "we want to", "i need to", "want to",
        "need to", "would like to", "help me", "the", "a", "an", "of", "in", "to", "for", "on",
        "and", "units", "unit", "pcs", "pieces", "stock", "inventory", "item", "items",
        "product", "products", "quantity", "qty", "quantity of", "how", "much", "many", "is",
        "are", "have", "has", "there", "do", "does", "me", "my", "our", "please", "kindly",
        "with", "from", "from supplier", "supplier",
        // Slot keywords. The value is already captured in its own field (and its own span), so
        // leaving the keyword behind leaks it into the product reference — which sent
        // "received 40 amox batch B9001 expiring 2027-05-01" to resolve as "amox expiring".
        "batch", "batch no", "batch#", "lot", "lot no", "lot number",
        "expiring", "expiry", "expires", "expiration", "expire", "best before",
        "price", "at price", "cost", "costing", "rate", "per unit", "buy",
        "received", "receive", "expiry date",
    ];
    let mut tokens: Vec<String> = remainder
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').to_string())
        .filter(|t| !t.is_empty() && !NOISE.contains(&t.as_str()) && t.len() > 1)
        // "hello, can you tell me..." is greeting, not a product. These made the reference
        // unresolvable, which is why the request fell through to a model that could not help.
        .filter(|t| !GREETING.contains(&t.as_str()))
        .collect();
    // "what about" style trailing question marks
    tokens.dedup();
    let product_query = if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    };
    intent.product_query = product_query;

    // Resolve the window for an expiry query from the phrase itself.
    if intent.action == Action::QueryExpiring {
        intent.within_days = intent.within_days.or_else(|| {
            if lower.contains("this week") {
                Some(7)
            } else if lower.contains("this month") {
                Some(30)
            } else if lower.contains("this year") {
                Some(365)
            } else if lower.contains("expired") || lower.contains("already") {
                Some(0)
            } else {
                Some(30)
            }
        });
    }

    if is_negative && intent.action == Action::Restock {
        // "did not add 40" — a negation must not mutate anything.
        intent.action = Action::QueryStock;
        intent.confidence = intent.confidence.min(0.4);
    }

    // A bare number plus a resolvable product with no verb is almost always a stock question.
    if intent.action == Action::Unknown && intent.product_query.is_some() {
        intent.action = Action::QueryStock;
        intent.confidence = 0.35;
    }

    intent
}

/// Resolve `product_query` against the catalogue. Returns the exact match, or a candidate list
/// for `did you mean` when the reference is ambiguous.
pub fn resolve(db: &Db, query: Option<&str>) -> AppResult<(Option<i64>, Vec<String>)> {
    let q = match query.map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => q,
        None => return Ok((None, Vec::new())),
    };
    let matches = db.read(|c| repo::search_products(c, q, 6))?;
    let names: Vec<String> = matches.iter().map(|m| m.name.clone()).collect();
    match matches.len() {
        0 => Ok((None, Vec::new())),
        // Only accept an exact name/SKU hit when the search did not also surface close
        // alternatives, so "amox" does not silently pick one of five Amoxicillin variants.
        1 => Ok((Some(matches[0].id), names)),
        _ => {
            let exact = matches.iter().position(|m| {
                m.name.eq_ignore_ascii_case(q) || m.sku.as_deref() == Some(q)
            });
            match exact {
                Some(i) => Ok((Some(matches[i].id), names)),
                None => Ok((None, names)),
            }
        }
    }
}

/// What the caller should do with a parse. Distinguishes "confidently actionable", "needs
/// disambiguation", and "hand it to the model".
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Deterministic path resolved the request. `missing` lists slots still required.
    Actionable { missing: Vec<&'static str> },
    /// A product reference matched more than one product.
    Ambiguous(Vec<String>),
    /// Not enough signal; the model should handle it.
    DeferToModel,
}

pub fn verdict(intent: &ParsedIntent, resolved: Option<i64>, candidates: &[String]) -> Verdict {
    const MIN: f64 = 0.70;
    if intent.confidence < MIN {
        return Verdict::DeferToModel;
    }
    if intent.action == Action::Unknown {
        return Verdict::DeferToModel;
    }
    // Read-only actions do not need a product, so an unresolved reference is not fatal.
    //
    // QueryStock is included only when a product was actually named. "How many products do I
    // have?" has no product reference and is a catalogue-wide question — routing it to the model
    // produced "I'm unable to provide that information", because the model has no rows to work
    // from until it decides to call a tool, and small models often just apologise instead.
    let needs_product = match intent.action {
        Action::Restock | Action::Sale | Action::Adjust => true,
        Action::QueryStock => intent.product_query.is_some(),
        _ => false,
    };
    if needs_product {
        match resolved {
            Some(_) => {}
            None => {
                if candidates.len() > 1 {
                    return Verdict::Ambiguous(candidates.to_vec());
                }
                return Verdict::DeferToModel;
            }
        }
    }
    let mut missing = intent.missing.clone();
    if intent.action == Action::Restock && intent.batch_no.is_none() {
        if !missing.contains(&"batch") {
            missing.push("batch");
        }
    }
    Verdict::Actionable { missing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_restock() {
        let i = parse("received 40 amoxicillin batch B2401 expiry 2027-03-01 at 12.50");
        assert_eq!(i.action, Action::Restock);
        assert_eq!(i.quantity, Some(40));
        assert_eq!(i.batch_no.as_deref(), Some("B2401"));
        assert_eq!(i.expiry.as_deref(), Some("2027-03-01"));
        assert_eq!(i.price, Some(12.5));
        assert!(i.product_query.as_deref().unwrap().contains("amoxicillin"));
    }

    /// AC-5. Every slot keyword must be consumed by its own field. When a keyword leaks into
    /// `product_query` — "amox expiring", "amox b2401 expiry" — product resolution fails and the
    /// write silently degrades to a model call, which is the exact failure mode AC-5 forbids.
    #[test]
    fn slot_keywords_never_leak_into_the_product_reference() {
        for (text, product, batch) in [
            (
                "received 40 amox batch B9001 expiring 2027-05-01 at 14.25",
                Some("amox"),
                Some("B9001"),
            ),
            (
                "add 40 amox b2401 expiry 2027-03-01 price 12.5",
                Some("amox"),
                Some("b2401"),
            ),
            (
                "received 40 amoxicillin B2401 expiring 2027-03-01 at 12.50",
                Some("amoxicillin"),
                Some("B2401"),
            ),
        ] {
            let i = parse(text);
            assert_eq!(i.product_query.as_deref(), product, "product ref for {text:?}");
            assert_eq!(i.batch_no.as_deref(), batch, "batch for {text:?}");
        }
    }

    /// A bare lot code with no label is the common operator shorthand.
    #[test]
    fn recognises_unlabelled_lot_codes() {
        let i = parse("stock in 12 panadol lot7");
        assert_eq!(i.batch_no.as_deref(), Some("lot7"));
        assert_eq!(i.product_query.as_deref(), Some("panadol"));
        assert_eq!(i.action, Action::Restock);
    }

    /// `lot` is checked after `batch`; a miss on the first label must not abandon the search.
    #[test]
    fn lot_label_resolves_when_batch_label_absent() {
        let i = parse("received 10 vitamin c lot 42");
        assert_eq!(i.batch_no.as_deref(), Some("42"));
        assert_eq!(i.quantity, Some(10));
    }

    /// Batch numbers are case-sensitive in the catalogue, so the original casing must survive.
    #[test]
    fn batch_casing_is_preserved_from_the_original_text() {
        assert_eq!(parse("received 5 amox B9001").batch_no.as_deref(), Some("B9001"));
        assert_eq!(parse("received 5 amox b9001").batch_no.as_deref(), Some("b9001"));
    }

    #[test]
    fn parses_sale() {
        let i = parse("sold 5 Panadol to Mr. Sharma");
        assert_eq!(i.action, Action::Sale);
        assert_eq!(i.quantity, Some(5));
        assert_eq!(i.buyer.as_deref(), Some("Sharma"));
        assert!(i.product_query.as_deref().unwrap().contains("panadol"));
    }

    #[test]
    fn parses_queries() {
        assert_eq!(parse("how many units of amox do I have").action, Action::QueryStock);
        assert_eq!(parse("what is expiring this month").action, Action::QueryExpiring);
        assert_eq!(parse("show inventory value").action, Action::QueryValue);
        assert_eq!(parse("recent sales history").action, Action::QueryHistory);
        assert_eq!(parse("who are my top buyers").action, Action::QueryBuyers);
    }

    #[test]
    fn negation_does_not_mutate() {
        let i = parse("I did not add 40 amoxicillin");
        assert_eq!(i.action, Action::QueryStock);
    }

    #[test]
    fn missing_quantity_lowers_confidence() {
        let i = parse("add amoxicillin batch B1");
        assert!(i.missing.contains(&"quantity"));
        assert!(i.confidence < 0.70);
    }

    #[test]
    fn verdict_defers_when_uncertain() {
        let i = parse("how many units of amox do I have");
        assert_eq!(verdict(&i, None, &[]), Verdict::DeferToModel);
    }

    /// Regression: "Bought Orymol V, Batch No 23456, Qty 450, Price Rs 40, Expiry 02 Oct 2027"
    /// was classified as an expiry *query* because it contains the word "expiry", so the agent
    /// replied "1 batch expiring within 30 days" and the 450-unit purchase was never recorded.
    /// A sentence with a write verb is a write, whatever slot words it also contains.
    #[test]
    fn write_verb_is_never_overridden_by_an_expiry_hint() {
        for text in [
            "Bought Orymol V , Batch No : 23456, Qty : 450, Price : Rs 40 , Expiry : 02 Oct 2027",
            "bought 40 amox batch B1 expiring 2027-03-01 at 12.50",
            "purchased 25 panadol lot7 expires 2028-01-01 at 9.99",
            "received 12 ORS sachets expiry 2027-09-30 price 18",
        ] {
            let i = parse(text);
            assert_eq!(
                i.action,
                Action::Restock,
                "{text:?} was classified as {:?}",
                i.action
            );
            assert!(i.quantity.is_some(), "{text:?} lost its quantity");
            assert!(i.expiry.is_some(), "{text:?} lost its expiry");
        }
        // A restock with no batch in the sentence is reported as missing, not silently dropped.
        let no_batch = parse("received 12 ORS sachets expiry 2027-09-30 price 18");
        assert!(no_batch.missing.contains(&"batch"));
    }

    /// Every slot in that purchase sentence must be recovered, including from the awkward
    /// `Batch No :` / `Qty :` form where the value is separated from its label by punctuation.
    #[test]
    fn colon_separated_slots_are_recovered() {
        let i = parse("Bought Orymol V , Batch No : 23456, Qty : 450, Price : Rs 40 , Expiry : 02 Oct 2027");
        assert_eq!(i.action, Action::Restock);
        assert_eq!(i.quantity, Some(450));
        assert_eq!(i.batch_no.as_deref(), Some("23456"));
        assert_eq!(i.price, Some(40.0));
        assert_eq!(i.expiry.as_deref(), Some("2027-10-02"));
        // The stray "no" from "Batch No" must not end up in the product reference.
        assert_eq!(i.product_query.as_deref(), Some("orymol"));
    }

    /// "Name me the products with the farthest expiry" is comparative. Answering it with a fixed
    /// 30-day window is confidently wrong, so it must reach the model.
    #[test]
    fn comparative_questions_defer_to_the_model() {
        for text in [
            "name me the products who have the farthest date for expiry",
            "which product has the longest shelf life",
            "compare the price of amox and panadol",
            "why did sales drop last week",
        ] {
            let i = parse(text);
            assert!(
                i.confidence < 0.70,
                "{text:?} should not be answerable deterministically (conf {})",
                i.confidence
            );
        }
    }

    /// Greeting and politeness are not part of a product name. "hello, can you tell me how many
    /// products i have" was resolving to the product reference "hello can you".
    #[test]
    fn conversational_filler_is_not_a_product_reference() {
        let i = parse("hello, can you tell me how many products i have in inventory");
        assert_eq!(i.action, Action::QueryStock);
        assert_eq!(
            i.product_query, None,
            "greeting leaked into the product reference"
        );
        // A catalogue-wide question is answerable, not a model deferral.
        assert_eq!(verdict(&i, None, &[]), Verdict::Actionable { missing: vec![] });
    }

    /// "Bought" is an inbound verb. Treating it as a sale (or ignoring it) loses the stock.
    #[test]
    fn purchase_verbs_are_inbound_not_outbound() {
        for text in ["bought 40 amox", "purchased 12 panadol", "procured 5 ORS"] {
            assert_eq!(parse(text).action, Action::Restock, "{text:?}");
        }
        // ...while the outbound verbs stay outbound.
        for text in ["sold 5 panadol", "dispatched 3 amox", "shipped 2 vials"] {
            assert_eq!(parse(text).action, Action::Sale, "{text:?}");
        }
    }
}
