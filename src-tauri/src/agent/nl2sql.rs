//! NL→SQL for read-only analytics. Defence in depth: a text validator, then an independent
//! connection opened `SQLITE_OPEN_READ_ONLY` with `PRAGMA query_only = ON`. Even a validator
//! bypass physically cannot write.

use crate::db::open_read_only;
use crate::error::{AppError, AppResult};
use crate::util;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::path::Path;

const ALLOWED_TABLES: &[&str] = &["products", "batches", "ledger", "usage_daily"];
const HARD_ROW_CAP: i64 = 200;

/// Any occurrence of a write/DDL keyword anywhere in the statement, including inside a string
/// literal. Stricter than a real parser on purpose.
static RE_FORBIDDEN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(INSERT|UPDATE|DELETE|DROP|ALTER|CREATE|ATTACH|DETACH|PRAGMA|VACUUM|REPLACE|REINDEX|TRIGGER|LOAD_EXTENSION|BEGIN|COMMIT|ROLLBACK|SAVEPOINT|ANALYZE)\b",
    )
    .unwrap()
});

/// `WITH name AS (` — CTE definitions introduce legitimate names that are not base tables.
static RE_CTE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b([a-z_][a-z0-9_]*)\s+AS\s*\(").unwrap());

/// `FROM tbl AS alias` / `JOIN tbl alias` — captures the alias.
static RE_ALIAS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:FROM|JOIN)\s+[a-z_][a-z0-9_]*\s+(?:AS\s+)?([a-z_][a-z0-9_]*)\b").unwrap()
});

static RE_FROM_JOIN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:FROM|JOIN)\s+([a-z_][a-z0-9_]*)").unwrap());

static RE_COMMENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"--[^\n]*|/\*.*?\*/").unwrap());

/// Strip string literals so keyword scanning cannot be fooled by quoted text.
fn mask_literals(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut in_str = false;
    let mut in_ident = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_str = !in_str;
                out.push(' ');
            }
            '"' if !in_str => {
                in_ident = !in_ident;
                out.push(' ');
            }
            '\\' if in_str => {
                chars.next();
                out.push(' ');
            }
            _ if in_str || in_ident => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Table aliases and CTE names introduced by the statement itself, which are legitimate but
/// must not be mistaken for base tables.
fn local_aliases(sql: &str) -> Vec<String> {
    let masked = mask_literals(sql);
    let mut names: Vec<String> = Vec::new();

    // CTE definitions: `WITH s AS (`, `WITH a AS (...), b AS (`
    for cap in RE_CTE.captures_iter(&masked) {
        names.push(cap[1].to_lowercase());
    }
    // Explicit and implicit aliases: `FROM batches AS b`, `FROM batches b`
    for cap in RE_ALIAS.captures_iter(&masked) {
        let alias = cap[1].to_lowercase();
        // Guard against swallowing a SQL keyword as an alias.
        if !matches!(
            alias.as_str(),
            "as" | "on" | "where" | "group" | "order" | "limit" | "join" | "inner" | "left"
                | "right" | "cross" | "and" | "or" | "select" | "having" | "union"
        ) {
            names.push(alias);
        }
    }
    names
}

/// Validate and normalise a generated query. Returns the SQL to execute.
pub fn validate(sql: &str) -> AppResult<String> {
    let mut cleaned = sql.trim().to_string();
    if cleaned.is_empty() {
        return Err(AppError::validation("the generated query was empty"));
    }

    // Strip comments before any other analysis so a comment cannot hide a keyword.
    cleaned = RE_COMMENT.replace_all(&cleaned, " ").to_string();
    cleaned = cleaned.trim().trim_end_matches(';').trim().to_string();

    // Exactly one statement. A `;` followed by anything other than whitespace is a second
    // statement and is rejected outright.
    if cleaned.contains(';') {
        return Err(AppError::validation(
            "only a single read-only statement is allowed",
        ));
    }

    let masked = mask_literals(&cleaned);
    let upper = masked.trim_start().to_uppercase();
    if !(upper.starts_with("SELECT") || upper.starts_with("WITH")) {
        return Err(AppError::validation(
            "only SELECT or WITH queries are allowed",
        ));
    }

    if let Some(m) = RE_FORBIDDEN.find(&masked) {
        return Err(AppError::validation(format!(
            "'{}' is not permitted in a read-only query",
            m.as_str().to_uppercase()
        )));
    }

    let aliases = local_aliases(&cleaned);
    for cap in RE_FROM_JOIN.captures_iter(&masked) {
        let table = cap[1].to_lowercase();
        if ALLOWED_TABLES.contains(&table.as_str()) || aliases.contains(&table) {
            continue;
        }
        return Err(AppError::validation(format!(
            "table '{table}' is not readable by the analytics layer"
        )));
    }

    // Force a hard row cap so a runaway `CROSS JOIN` cannot exhaust memory.
    let has_limit = Regex::new(r"(?i)\bLIMIT\s+\d+")
        .unwrap()
        .is_match(&masked);
    let capped = if has_limit {
        cleaned
    } else {
        format!("{cleaned} LIMIT {HARD_ROW_CAP}")
    };

    if capped.len() > 4000 {
        return Err(AppError::validation("the generated query is too long"));
    }
    Ok(capped)
}

/// Execute a validated query on an independent read-only connection.
pub fn run(db_path: &Path, sql: &str) -> AppResult<(Vec<String>, Vec<Vec<Value>>)> {
    let safe = validate(sql)?;
    let conn = open_read_only(db_path)?;
    let mut stmt = conn
        .prepare(&safe)
        .map_err(|e| AppError::validation(format!("could not prepare query: {e}")))?;

    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let column_count = names.len();

    let mut rows_out: Vec<Vec<Value>> = Vec::new();
    let mut rows = stmt.query([]).map_err(|e| {
        AppError::validation(format!("query failed: {e}"))
    })?;

    while let Some(row) = rows
        .next()
        .map_err(|e| AppError::validation(format!("row error: {e}")))?
    {
        if rows_out.len() as i64 >= HARD_ROW_CAP {
            break;
        }
        let mut vals = Vec::with_capacity(column_count);
        for i in 0..column_count {
            let v = match row.get_ref(i) {
                Ok(rusqlite::types::ValueRef::Null) => Value::Null,
                Ok(rusqlite::types::ValueRef::Integer(n)) => Value::from(n),
                Ok(rusqlite::types::ValueRef::Real(f)) => Value::from(f),
                Ok(rusqlite::types::ValueRef::Text(t)) => {
                    Value::from(String::from_utf8_lossy(t).to_string())
                }
                Ok(rusqlite::types::ValueRef::Blob(_)) => Value::Null,
                Err(_) => Value::Null,
            };
            vals.push(v);
        }
        rows_out.push(vals);
    }
    Ok((names, rows_out))
}

/// Schema summary handed to the model so it does not have to guess column names.
pub fn schema_prompt() -> String {
    format!(
        "products(id, name, sku, barcode, category, unit, description, reorder_point, \
         lead_time_days, created_at, updated_at)\n\
         batches(id, product_id, batch_no, expiry_date, unit_price, quantity, received_at, \
         supplier, notes)\n\
         ledger(id, ts, kind, product_id, product_name, batch_id, batch_no, expiry_date, \
         qty_delta, unit_price, total_value, buyer_name, note, source, actor)\n\
         usage_daily(product_id, day, units_out, units_in)\n\
         \n\
         Readable tables: {}. Queries are capped at {} rows.",
        ALLOWED_TABLES.join(", "),
        HARD_ROW_CAP
    )
}

/// Render a result set as a compact markdown table for the model and the UI.
pub fn render(names: &[String], rows: &[Vec<Value>]) -> String {
    if rows.is_empty() {
        return "No matching rows.".into();
    }
    let mut out = String::new();
    out.push('|');
    out.push_str(&names.join(" | "));
    out.push_str("|\n|");
    out.push_str(&names.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));
    out.push_str("|\n");
    for r in rows.iter().take(40) {
        out.push('|');
        let cells: Vec<String> = r
            .iter()
            .map(|v| {
                let s = match v {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                util::truncate_chars(&s.replace('|', "/"), 40)
            })
            .collect();
        out.push_str(&cells.join(" | "));
        out.push_str("|\n");
    }
    if rows.len() > 40 {
        out.push_str(&format!("… {} more rows\n", rows.len() - 40));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_writes() {
        for bad in [
            "DELETE FROM ledger",
            "UPDATE products SET name='x'",
            "INSERT INTO products VALUES (1)",
            "DROP TABLE products",
            "SELECT 1; DELETE FROM ledger",
            "SELECT * FROM sqlite_master",
            "PRAGMA table_info(products)",
        ] {
            assert!(validate(bad).is_err(), "should have rejected: {bad}");
        }
    }

    #[test]
    fn accepts_reads_and_caps() {
        let ok = validate("SELECT name FROM products ORDER BY name").unwrap();
        assert!(ok.contains("LIMIT 200"));
        let ok2 = validate("SELECT * FROM batches LIMIT 5").unwrap();
        assert!(ok2.contains("LIMIT 5"));
        assert!(!ok2.contains("LIMIT 200"));
    }

    #[test]
    fn accepts_cte() {
        let ok = validate(
            "WITH s AS (SELECT product_id, SUM(quantity) q FROM batches GROUP BY product_id) \
             SELECT p.name, s.q FROM s JOIN products p ON p.id = s.product_id",
        );
        assert!(ok.is_ok(), "{:?}", validate("WITH s AS (SELECT 1 a FROM products) SELECT * FROM s"));
    }

    #[test]
    fn keyword_in_literal_is_masked() {
        // A string containing 'delete' must not trip the keyword scan.
        let q = validate("SELECT note FROM ledger WHERE note = 'please delete this'").unwrap();
        assert!(q.contains("SELECT"));
    }

    #[test]
    fn keyword_in_comment_is_masked() {
        // Comments are stripped before the scan, so a keyword inside one is inert.
        assert!(validate("SELECT 1 AS n -- DROP TABLE ledger").is_ok());
        assert!(validate("/* UPDATE ledger */ SELECT 1 AS n").is_ok());
        // ...but a real keyword after a comment is still caught.
        assert!(validate("SELECT 1 AS n -- a comment\nDROP TABLE ledger").is_err());
    }

    #[test]
    fn only_every_allowed_table_is_readable() {
        for t in ALLOWED_TABLES {
            assert!(
                validate(&format!("SELECT * FROM {t} LIMIT 1")).is_ok(),
                "{t} is on the allowlist but was rejected"
            );
        }
        // Reachable-but-not-allowlisted internals must stay closed.
        for t in ["sqlite_master", "pragma_table_info", "fts5_master", "settings"] {
            if ALLOWED_TABLES.contains(&t) {
                continue;
            }
            assert!(
                validate(&format!("SELECT * FROM {t}")).is_err(),
                "{t} must not be readable"
            );
        }
    }

    #[test]
    fn comment_hidden_table_name_is_still_rejected() {
        assert!(validate("SELECT 1 FROM products JOIN /*x*/ sqlite_master ON 1=1").is_err());
    }

    #[test]
    fn limit_is_forced_and_cannot_be_removed() {
        assert!(validate("SELECT * FROM ledger").unwrap().contains("LIMIT 200"));
        // An existing limit is respected rather than doubled.
        assert_eq!(validate("SELECT * FROM ledger LIMIT 7").unwrap().matches("LIMIT").count(), 1);
        // A LIMIT hidden inside a string literal must not suppress the real cap.
        let q = validate("SELECT 'LIMIT 99999' AS note FROM ledger").unwrap();
        assert!(q.contains(&format!("LIMIT {HARD_ROW_CAP}")), "got {q}");
    }

    #[test]
    fn oversized_statements_are_refused() {
        let huge = format!(
            "SELECT {} FROM ledger",
            std::iter::repeat("note,").take(2000).collect::<Vec<_>>().join("")
        );
        assert!(validate(&huge).is_err(), "a 10 KB statement should be refused");
    }

    #[test]
    fn query_only_connection_refuses_writes_even_if_the_validator_is_bypassed() {
        // Defence in depth: the validator is the first gate, but the execution connection is
        // opened read-only with `PRAGMA query_only`, so a bypass still cannot write.
        let dir = std::env::temp_dir().join("stockpile-nl2sql-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("guard.db");

        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS ledger(id INTEGER PRIMARY KEY, note TEXT);\
                 INSERT INTO ledger(id, note) VALUES (1, 'untouched');",
            )
            .unwrap();
        }

        let conn = open_read_only(&path).unwrap();
        conn.prepare("UPDATE ledger SET note = 'hacked'").unwrap().execute([]).unwrap_err();
        let note: String = conn
            .query_row("SELECT note FROM ledger WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(note, "untouched", "the read-only connection must reject the write");

        // And a write that gets past prepare must fail at execute time under query_only.
        conn.prepare("INSERT INTO ledger(id, note) VALUES (2, 'x')")
            .unwrap()
            .execute([])
            .unwrap_err();

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn nested_double_statement_is_rejected() {
        for bad in [
            "SELECT 1; DROP TABLE products",
            "SELECT 1 ; SELECT 2",
            "WITH a AS (SELECT 1 x) SELECT * FROM a; WITH b AS (SELECT 2 y) SELECT * FROM b",
        ] {
            assert!(validate(bad).is_err(), "should have rejected: {bad}");
        }
        // A single trailing semicolon is tolerated and stripped.
        assert!(validate("SELECT 1 FROM products;").is_ok());
    }
}