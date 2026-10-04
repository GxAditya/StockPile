use super::Capabilities;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::util::{self, fts_terms, trigram_similarity};
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, Row};

const EXPIRY_WARN_DAYS: i64 = 30;

/// Columns exposed to SQL `ORDER BY`, whitelisted — never interpolate user input.
fn sort_expr(key: &str) -> &'static str {
    match key {
        "name" => "s.name COLLATE NOCASE ASC",
        "category" => "s.category COLLATE NOCASE ASC, s.name COLLATE NOCASE ASC",
        "totalQuantity" => "s.total_quantity DESC",
        "totalValue" => "s.total_value DESC",
        "batchCount" => "s.batch_count DESC",
        "avgCost" => "s.avg_cost DESC",
        "earliestExpiry" => "s.earliest_expiry IS NULL ASC, s.earliest_expiry ASC",
        // Placeholder expanded to HEALTH_RANK below, so problems sort first.
        "health" => "__HEALTH__ ASC, s.name COLLATE NOCASE ASC",
        _ => "s.name COLLATE NOCASE ASC",
    }
}

/// Health ordering that puts problems first regardless of alphabetical health name.
const HEALTH_RANK: &str = "CASE s.health WHEN 'expired' THEN 0 WHEN 'expiring' THEN 1 \
     WHEN 'out' THEN 2 WHEN 'low' THEN 3 ELSE 4 END";

const STOCK_CTE: &str = r#"
WITH agg AS (
    SELECT
        p.id, p.name, p.sku, p.barcode, p.category, p.unit, p.description,
        p.reorder_point, p.lead_time_days,
        COALESCE(SUM(b.quantity), 0)                                        AS total_quantity,
        COALESCE(SUM(CASE WHEN b.quantity > 0 THEN 1 ELSE 0 END), 0)         AS batch_count,
        COALESCE(SUM(b.quantity * b.unit_price), 0)                         AS total_value,
        MIN(CASE WHEN b.quantity > 0 THEN b.expiry_date END)                AS earliest_expiry,
        COALESCE(SUM(CASE WHEN b.quantity > 0 AND b.expiry_date IS NOT NULL
                          AND b.expiry_date < :today THEN b.quantity ELSE 0 END), 0) AS expired_units,
        COALESCE(SUM(CASE WHEN b.quantity > 0 AND b.expiry_date IS NOT NULL
                          AND b.expiry_date >= :today AND b.expiry_date <= :soon
                          THEN b.quantity ELSE 0 END), 0)                  AS expiring_units
    FROM products p
    LEFT JOIN batches b ON b.product_id = p.id
    GROUP BY p.id
),
moves AS (
    SELECT product_id, MAX(ts) AS last_ts FROM ledger GROUP BY product_id
)
"#;

fn stock_select() -> String {
    format!(
        "{STOCK_CTE}
         SELECT s.id, s.name, s.sku, s.barcode, s.category, s.unit, s.description,
                s.reorder_point, s.lead_time_days,
                s.total_quantity, s.batch_count, s.total_value, s.earliest_expiry,
                s.expired_units, s.expiring_units, m.last_ts,
                CASE WHEN s.total_quantity > 0
                     THEN s.total_value / s.total_quantity ELSE 0 END AS avg_cost,
                CASE
                    WHEN s.expired_units > 0     THEN 'expired'
                    WHEN s.expiring_units > 0    THEN 'expiring'
                    WHEN s.total_quantity = 0    THEN 'out'
                    WHEN s.total_quantity <= s.reorder_point THEN 'low'
                    ELSE 'ok'
                END AS health
         FROM agg s
         LEFT JOIN moves m ON m.product_id = s.id
         WHERE 1=1"
    )
}

pub struct StockQuery {
    pub search: Option<String>,
    pub category: Option<String>,
    pub health: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Columns are read **by name**, never by position. `stock_select()` is assembled from a CTE
/// plus a wide projection, and positional indices silently drift the moment that projection is
/// edited — which is exactly the bug this replaces (the mapper had been reading `health` from
/// index 18 of an 18-column result).
fn map_stock_row(r: &Row<'_>) -> rusqlite::Result<StockRow> {
    let health_s: String = r.get("health")?;
    Ok(StockRow {
        id: r.get("id")?,
        name: r.get("name")?,
        sku: r.get("sku")?,
        barcode: r.get("barcode")?,
        category: r.get("category")?,
        unit: r.get("unit")?,
        description: r.get("description")?,
        reorder_point: r.get("reorder_point")?,
        lead_time_days: r.get("lead_time_days")?,
        total_quantity: r.get("total_quantity")?,
        batch_count: r.get("batch_count")?,
        avg_cost: r.get("avg_cost")?,
        total_value: r.get("total_value")?,
        earliest_expiry: r.get("earliest_expiry")?,
        expired_units: r.get("expired_units")?,
        expiring_units: r.get("expiring_units")?,
        health: match health_s.as_str() {
            "expired" => StockHealth::Expired,
            "expiring" => StockHealth::Expiring,
            "out" => StockHealth::Out,
            "low" => StockHealth::Low,
            _ => StockHealth::Ok,
        },
        last_movement_at: r.get("last_ts")?,
    })
}

pub fn stock_list(conn: &Connection, q: &StockQuery) -> AppResult<StockList> {
    let today = util::today_str();
    let soon = util::add_days(util::today(), EXPIRY_WARN_DAYS);
    let caps = Capabilities {
        fts5: probe_fts(conn),
    };

    let mut params: Vec<Value> = vec![today.clone().into(), soon.clone().into()];
    let mut where_sql = String::new();

    if let Some(cat) = q.category.as_deref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND s.category = ?");
        params.push(cat.to_string().into());
    }
    if let Some(h) = q.health.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        where_sql.push_str(" AND health = ?");
        params.push(h.to_string().into());
    }
    if let Some(search) = q.search.as_deref().filter(|s| !s.trim().is_empty()) {
        let like = format!("%{}%", search.trim());
        if caps.fts5 {
            let terms = fts_terms(search);
            if !terms.is_empty() {
                where_sql.push_str(
                    " AND s.id IN (SELECT rowid FROM products_fts WHERE products_fts MATCH ?)",
                );
                params.push(terms.join(" ").into());
            } else {
                where_sql.push_str(" AND (s.name LIKE ? OR s.sku LIKE ? OR s.category LIKE ?)");
                params.push(like.clone().into());
                params.push(like.clone().into());
                params.push(like.into());
            }
        } else {
            where_sql.push_str(" AND (s.name LIKE ? OR s.sku LIKE ? OR s.category LIKE ?)");
            params.push(like.clone().into());
            params.push(like.clone().into());
            params.push(like.into());
        }
    }

    let order = format!(
        " ORDER BY {} ",
        sort_expr(q.sort.as_deref().unwrap_or("name")).replace("__HEALTH__", HEALTH_RANK)
    );

    // Count over the same predicate set.
    let count_sql = format!("SELECT COUNT(*) FROM ({}{})", stock_select(), where_sql);
    let total: i64 = conn.query_row(&count_sql, params_from_iter(params.clone()), |r| r.get(0))?;

    let mut paged = params.clone();
    let sql = format!("{}{}{} LIMIT ? OFFSET ?", stock_select(), where_sql, order);
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    paged.push((limit as i64).into());
    paged.push(q.offset.unwrap_or(0).max(0).into());

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(paged), map_stock_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let grand_total_units: i64 = conn.query_row(
        &format!("SELECT COALESCE(SUM(s.total_quantity),0) FROM ({}) s", stock_select()),
        params_from_iter(params.clone()),
        |r| r.get(0),
    )?;
    let grand_total_value: f64 = conn.query_row(
        &format!("SELECT COALESCE(SUM(s.total_value),0) FROM ({}) s", stock_select()),
        params_from_iter(params),
        |r| r.get(0),
    )?;

    Ok(StockList {
        rows,
        total: total.max(0) as usize,
        grand_total_units,
        grand_total_value,
    })
}

fn probe_fts(conn: &Connection) -> bool {
    conn.query_row("SELECT count(*) FROM products_fts", [], |r| r.get::<_, i64>(0))
        .is_ok()
}

// ── Products ────────────────────────────────────────────────────────────────────────────

fn map_product(r: &Row<'_>) -> rusqlite::Result<Product> {
    Ok(Product {
        id: r.get(0)?,
        name: r.get(1)?,
        sku: r.get(2)?,
        barcode: r.get(3)?,
        category: r.get(4)?,
        unit: r.get(5)?,
        description: r.get(6)?,
        reorder_point: r.get(7)?,
        lead_time_days: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const PRODUCT_COLS: &str = "id, name, sku, barcode, category, unit, description, \
     reorder_point, lead_time_days, created_at, updated_at";

pub fn find_product(conn: &Connection, id: i64) -> AppResult<Product> {
    conn.query_row(
        &format!("SELECT {PRODUCT_COLS} FROM products WHERE id = ?"),
        [id],
        map_product,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!("product {id} not found")),
        other => AppError::Db(other),
    })
}

pub fn find_product_by_name(conn: &Connection, name: &str) -> AppResult<Option<Product>> {
    Ok(conn
        .query_row(
            &format!("SELECT {PRODUCT_COLS} FROM products WHERE name = ? COLLATE NOCASE"),
            [name.trim()],
            map_product,
        )
        .ok())
}

pub fn create_product(conn: &Connection, p: &NewProduct) -> AppResult<Product> {
    let name = p
        .name
        .trim()
        .replace(char::is_whitespace, " ")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(AppError::validation("product name is required"));
    }
    if name.chars().count() > 160 {
        return Err(AppError::validation("product name is too long (max 160 characters)"));
    }

    let sku = util::clean_opt(p.sku.as_deref());
    if let Some(s) = &sku {
        if s.chars().count() > 64 {
            return Err(AppError::validation("SKU is too long (max 64 characters)"));
        }
        if let Some(_clash) = conn
            .query_row("SELECT id FROM products WHERE sku = ?", [s], |r| r.get::<_, i64>(0))
            .ok()
        {
            return Err(AppError::Conflict(format!("SKU '{s}' is already in use")));
        }
    }
    if find_product_by_name(conn, &name)?.is_some() {
        return Err(AppError::Conflict(format!(
            "a product named '{name}' already exists"
        )));
    }

    let unit = p
        .unit
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_UNIT)
        .to_string();
    let reorder = p.reorder_point.unwrap_or(0.0);
    if !reorder.is_finite() || reorder < 0.0 {
        return Err(AppError::validation("reorder point must be zero or positive"));
    }
    let lead = p.lead_time_days.unwrap_or(7);
    if !(0..=3650).contains(&lead) {
        return Err(AppError::validation("lead time must be between 0 and 3650 days"));
    }

    let now = util::now_ts();
    conn.execute(
        "INSERT INTO products (name, sku, barcode, category, unit, description,
                               reorder_point, lead_time_days, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
        rusqlite::params![
            name,
            sku,
            util::clean_opt(p.barcode.as_deref()),
            util::clean_opt(p.category.as_deref()),
            unit,
            util::clean_opt(p.description.as_deref()),
            reorder,
            lead,
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Product {
        id,
        name,
        sku,
        barcode: util::clean_opt(p.barcode.as_deref()),
        category: util::clean_opt(p.category.as_deref()),
        unit,
        description: util::clean_opt(p.description.as_deref()),
        reorder_point: reorder,
        lead_time_days: lead,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub fn update_product(conn: &Connection, id: i64, p: &NewProduct) -> AppResult<Product> {
    let existing = find_product(conn, id)?;
    let name = p
        .name
        .trim()
        .replace(char::is_whitespace, " ")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(AppError::validation("product name is required"));
    }
    if let Some(other) = find_product_by_name(conn, &name)? {
        if other.id != id {
            return Err(AppError::Conflict(format!(
                "a product named '{name}' already exists"
            )));
        }
    }
    let sku = util::clean_opt(p.sku.as_deref());
    let reorder = p.reorder_point.unwrap_or(existing.reorder_point);
    let lead = p.lead_time_days.unwrap_or(existing.lead_time_days);
    if !reorder.is_finite() || reorder < 0.0 {
        return Err(AppError::validation("reorder point must be zero or positive"));
    }
    if !(0..=3650).contains(&lead) {
        return Err(AppError::validation("lead time must be between 0 and 3650 days"));
    }

    let unit = p
        .unit
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.unit)
        .to_string();

    conn.execute(
        "UPDATE products SET name=?1, sku=?2, barcode=?3, category=?4, unit=?5,
                description=?6, reorder_point=?7, lead_time_days=?8, updated_at=?9
         WHERE id=?10",
        rusqlite::params![
            name,
            sku,
            util::clean_opt(p.barcode.as_deref()),
            util::clean_opt(p.category.as_deref()),
            unit,
            util::clean_opt(p.description.as_deref()),
            reorder,
            lead,
            util::now_ts(),
            id
        ],
    )?;
    find_product(conn, id)
}

pub fn all_products(conn: &Connection) -> AppResult<Vec<Product>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {PRODUCT_COLS} FROM products ORDER BY name COLLATE NOCASE"))?;
    let rows = stmt
        .query_map([], map_product)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn categories(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT category FROM products
         WHERE category IS NOT NULL AND category <> ''
         ORDER BY category COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// FTS5-backed prefix search with a trigram fallback so typos still resolve.
/// Re-ranking by trigram similarity means "amoxcillin" finds "Amoxicillin".
pub fn search_products(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<Product>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let caps = Capabilities { fts5: probe_fts(conn) };
    let mut found: Vec<Product> = Vec::new();

    if caps.fts5 {
        let terms = fts_terms(q);
        if !terms.is_empty() {
            if let Ok(mut stmt) = conn.prepare(&format!(
                "SELECT {PRODUCT_COLS} FROM products
                 WHERE id IN (SELECT rowid FROM products_fts WHERE products_fts MATCH ?)
                 LIMIT ?"
            )) {
                let p: Vec<Value> = vec![terms.join(" ").into(), limit.into()];
                if let Ok(rows) = stmt.query_map(params_from_iter(p), map_product) {
                    found = rows.collect::<rusqlite::Result<Vec<_>>>().unwrap_or_default();
                }
            }
        }
    }

    if found.is_empty() {
        let like = format!("%{}%", q);
        if let Ok(mut stmt) = conn.prepare(&format!(
            "SELECT {PRODUCT_COLS} FROM products
             WHERE name LIKE ?1 OR sku LIKE ?1 OR category LIKE ?1
             ORDER BY CASE WHEN name LIKE ?2 THEN 0 ELSE 1 END, name COLLATE NOCASE
             LIMIT ?3"
        )) {
            let p: Vec<Value> = vec![like.clone().into(), format!("{q}%").into(), limit.into()];
            if let Ok(rows) = stmt.query_map(params_from_iter(p), map_product) {
                found = rows.collect::<rusqlite::Result<Vec<_>>>().unwrap_or_default();
            }
        }
    }

    if found.is_empty() {
        // Last resort: scan names for trigram overlap. Bounded scan; catalogue-sized by design.
        if let Ok(all) = all_products(conn) {
            let mut scored: Vec<(f64, Product)> = all
                .into_iter()
                .map(|p| {
                    let s = trigram_similarity(q, &p.name);
                    (s, p)
                })
                .filter(|(s, _)| *s >= 0.34)
                .collect();
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            found = scored.into_iter().take(limit as usize).map(|(_, p)| p).collect();
        }
    }

    Ok(found)
}

// ── Batches ─────────────────────────────────────────────────────────────────────────────

fn map_batch(r: &Row<'_>) -> rusqlite::Result<Batch> {
    Ok(Batch {
        id: r.get(0)?,
        product_id: r.get(1)?,
        batch_no: r.get(2)?,
        expiry_date: r.get(3)?,
        unit_price: r.get(4)?,
        quantity: r.get(5)?,
        received_at: r.get(6)?,
        supplier: r.get(7)?,
        notes: r.get(8)?,
        product_name: r.get(9).ok(),
        unit: r.get(10).ok(),
    })
}

const BATCH_COLS: &str = "b.id, b.product_id, b.batch_no, b.expiry_date, b.unit_price, \
     b.quantity, b.received_at, b.supplier, b.notes";

pub fn list_batches(conn: &Connection, product_id: i64) -> AppResult<Vec<Batch>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {BATCH_COLS}, p.name, p.unit FROM batches b
         JOIN products p ON p.id = b.product_id
         WHERE b.product_id = ?1
         ORDER BY (b.quantity > 0) DESC,
                  CASE WHEN b.expiry_date IS NULL THEN 1 ELSE 0 END ASC,
                  b.expiry_date ASC, b.id ASC"
    ))?;
    let rows = stmt
        .query_map([product_id], map_batch)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn find_batch(conn: &Connection, product_id: i64, batch_no: &str) -> AppResult<Option<Batch>> {
    let p: Vec<Value> = vec![product_id.into(), batch_no.trim().to_string().into()];
    Ok(conn
        .query_row(
            &format!(
                "SELECT {BATCH_COLS}, p.name, p.unit FROM batches b
                 JOIN products p ON p.id = b.product_id
                 WHERE b.product_id = ?1 AND b.batch_no = ?2"
            ),
            params_from_iter(p),
            map_batch,
        )
        .ok())
}

/// Live batches in FEFO order. Expired batches sort last so they are only drawn when allowed.
pub fn live_batches_fefo(conn: &Connection, product_id: i64) -> AppResult<Vec<Batch>> {
    let today = util::today_str();
    let mut stmt = conn.prepare(&format!(
        "SELECT {BATCH_COLS}, p.name, p.unit FROM batches b
         JOIN products p ON p.id = b.product_id
         WHERE b.product_id = ?1 AND b.quantity > 0
         ORDER BY CASE WHEN b.expiry_date IS NULL THEN 1 ELSE 0 END ASC,
                  CASE WHEN b.expiry_date IS NOT NULL AND b.expiry_date < ?2 THEN 1 ELSE 0 END ASC,
                  b.expiry_date ASC, b.unit_price ASC, b.id ASC"
    ))?;
    let p: Vec<Value> = vec![product_id.into(), today.into()];
    let rows = stmt
        .query_map(params_from_iter(p), map_batch)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn batch_count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT count(*) FROM batches WHERE quantity > 0", [], |r| r.get(0))?)
}

// ── Ledger ──────────────────────────────────────────────────────────────────────────────

pub struct LedgerRow {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub product_id: i64,
    pub product_name: String,
    pub batch_id: Option<i64>,
    pub batch_no: Option<String>,
    pub expiry_date: Option<String>,
    pub qty_delta: i64,
    pub unit_price: f64,
    pub total_value: f64,
    pub buyer_name: Option<String>,
    pub note: Option<String>,
    pub source: String,
    pub actor: Option<String>,
}

impl From<LedgerRow> for LedgerEntry {
    fn from(r: LedgerRow) -> Self {
        LedgerEntry {
            id: r.id,
            ts: r.ts,
            kind: r.kind,
            product_id: r.product_id,
            product_name: r.product_name,
            batch_id: r.batch_id,
            batch_no: r.batch_no,
            expiry_date: r.expiry_date,
            qty_delta: r.qty_delta,
            unit_price: r.unit_price,
            total_value: r.total_value,
            buyer_name: r.buyer_name,
            note: r.note,
            source: r.source,
            actor: r.actor,
        }
    }
}

fn map_ledger(r: &Row<'_>) -> rusqlite::Result<LedgerRow> {
    Ok(LedgerRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        kind: r.get(2)?,
        product_id: r.get(3)?,
        product_name: r.get(4)?,
        batch_id: r.get(5)?,
        batch_no: r.get(6)?,
        expiry_date: r.get(7)?,
        qty_delta: r.get(8)?,
        unit_price: r.get(9)?,
        total_value: r.get(10)?,
        buyer_name: r.get(11)?,
        note: r.get(12)?,
        source: r.get(13)?,
        actor: r.get(14)?,
    })
}

const LEDGER_COLS: &str = "id, ts, kind, product_id, product_name, batch_id, batch_no, \
     expiry_date, qty_delta, unit_price, total_value, buyer_name, note, source, actor";

/// Append-only. No code path anywhere issues UPDATE or DELETE against `ledger`.
pub fn append_ledger(conn: &Connection, row: &LedgerRow) -> AppResult<i64> {
    // `total_value` is a magnitude. Normalise defensively rather than trusting every caller to
    // remember, so a future negative write cannot silently invert revenue.
    let total_value = row.total_value.abs();
    let p: Vec<Value> = vec![
        row.ts.clone().into(),
        row.kind.clone().into(),
        row.product_id.into(),
        row.product_name.clone().into(),
        row.batch_id.into(),
        row.batch_no.clone().into(),
        row.expiry_date.clone().into(),
        row.qty_delta.into(),
        row.unit_price.into(),
        total_value.into(),
        row.buyer_name.clone().into(),
        row.note.clone().into(),
        row.source.clone().into(),
        row.actor.clone().into(),
    ];
    conn.execute(
        "INSERT INTO ledger (ts, kind, product_id, product_name, batch_id, batch_no,
                             expiry_date, qty_delta, unit_price, total_value,
                             buyer_name, note, source, actor)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params_from_iter(p),
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn bump_usage_day(
    conn: &Connection,
    product_id: i64,
    day: &str,
    out: i64,
    in_: i64,
) -> AppResult<()> {
    let p: Vec<Value> = vec![product_id.into(), day.to_string().into(), out.into(), in_.into()];
    conn.execute(
        "INSERT INTO usage_daily (product_id, day, units_out, units_in)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(product_id, day) DO UPDATE SET
             units_out = units_out + excluded.units_out,
             units_in  = units_in  + excluded.units_in",
        params_from_iter(p),
    )?;
    Ok(())
}

pub fn history(conn: &Connection, f: &HistoryFilter) -> AppResult<HistoryPage> {
    let mut where_sql = String::from(" WHERE 1=1");
    let mut params: Vec<Value> = Vec::new();

    if let Some(kinds) = f.kinds.as_ref().filter(|k| !k.is_empty()) {
        let valid: Vec<&str> = kinds
            .iter()
            .filter_map(|k| LedgerKind::parse(k).map(|p| p.as_str()))
            .collect();
        if !valid.is_empty() {
            where_sql.push_str(&format!(
                " AND kind IN ({})",
                valid.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            ));
            for k in valid {
                params.push(k.to_string().into());
            }
        }
    }
    if let Some(pid) = f.product_id {
        where_sql.push_str(" AND product_id = ?");
        params.push(pid.into());
    }
    if let Some(buyer) = f.buyer_name.as_deref().filter(|s| !s.trim().is_empty()) {
        where_sql.push_str(" AND buyer_name = ? COLLATE NOCASE");
        params.push(buyer.trim().to_string().into());
    }
    if let Some(from) = f.from.as_deref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND substr(ts,1,10) >= ?");
        params.push(from.to_string().into());
    }
    if let Some(to) = f.to.as_deref().filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND substr(ts,1,10) <= ?");
        params.push(to.to_string().into());
    }
    if let Some(search) = f.search.as_deref().filter(|s| !s.trim().is_empty()) {
        let like = format!("%{}%", search.trim());
        where_sql.push_str(
            " AND (product_name LIKE ?1 OR batch_no LIKE ?1 OR buyer_name LIKE ?1 OR note LIKE ?1)",
        );
        params.push(like.into());
    }

    let total_rows: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM ledger{where_sql}"),
        params_from_iter(params.clone()),
        |r| r.get(0),
    )?;

    let (units_in, units_out, value_in, value_out): (i64, i64, f64, f64) = conn.query_row(
        &format!(
            "SELECT
                 COALESCE(SUM(CASE WHEN qty_delta > 0 THEN qty_delta ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN qty_delta < 0 THEN -qty_delta ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN qty_delta > 0 THEN total_value ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN qty_delta < 0 THEN total_value ELSE 0 END), 0)
             FROM ledger{where_sql}"
        ),
        params_from_iter(params.clone()),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;

    let limit = f.limit.unwrap_or(200).clamp(1, 2000);
    let offset = f.offset.unwrap_or(0).max(0);
    let mut paged = params;
    paged.push(limit.into());
    paged.push(offset.into());

    let mut stmt = conn.prepare(&format!(
        "SELECT {LEDGER_COLS} FROM ledger{where_sql} ORDER BY ts DESC, id DESC LIMIT ? OFFSET ?"
    ))?;
    let flat = stmt
        .query_map(params_from_iter(paged), map_ledger)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut days: Vec<HistoryDay> = Vec::new();
    for row in flat {
        let entry: LedgerEntry = row.into();
        let day = entry.ts.chars().take(10).collect::<String>();
        match days.iter_mut().find(|d| d.date == day) {
            Some(existing) => {
                if entry.qty_delta > 0 {
                    existing.units_in += entry.qty_delta;
                    existing.value_in += entry.total_value;
                } else {
                    existing.units_out += -entry.qty_delta;
                    existing.value_out += entry.total_value;
                }
                existing.entries.push(entry);
            }
            None => {
                let (ui, uo, vi, vo) = if entry.qty_delta > 0 {
                    (entry.qty_delta, 0, entry.total_value, 0.0)
                } else {
                    (0, -entry.qty_delta, 0.0, entry.total_value)
                };
                days.push(HistoryDay {
                    date: day,
                    units_in: ui,
                    units_out: uo,
                    value_in: vi,
                    value_out: vo,
                    entries: vec![entry],
                });
            }
        }
    }

    let mut buyers_stmt =
        conn.prepare("SELECT DISTINCT buyer_name FROM ledger WHERE kind='sale' AND buyer_name IS NOT NULL AND buyer_name <> '' ORDER BY buyer_name COLLATE NOCASE")?;
    let buyers = buyers_stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(HistoryPage {
        days,
        total_rows,
        units_in,
        units_out,
        value_in,
        value_out,
        buyers,
    })
}

/// Flat ledger for CSV export. Identical predicate set as `history`.
pub fn history_flat(conn: &Connection, f: &HistoryFilter) -> AppResult<Vec<LedgerEntry>> {
    let page = history(
        conn,
        &HistoryFilter {
            limit: Some(100_000),
            offset: Some(0),
            ..f.clone()
        },
    )?;
    Ok(page.days.into_iter().flat_map(|d| d.entries).collect())
}

pub fn product_detail(conn: &Connection, id: i64) -> AppResult<ProductDetail> {
    let product = find_product(conn, id)?;
    let batches = list_batches(conn, id)?;

    // `stock_list` already filters to products with no stock-health predicate, so reuse it and
    // pick this product's row rather than re-deriving the aggregate CTE.
    let list = stock_list(
        conn,
        &StockQuery {
            search: None,
            category: None,
            health: None,
            sort: None,
            limit: Some(5000),
            offset: None,
        },
    )?;
    let stock = list
        .rows
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| AppError::NotFound(format!("product {id} not found")))?;

    Ok(ProductDetail {
        product,
        stock,
        batches,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::domain;
    use crate::models::{NewBatch, NewProduct, RestockRequest, SaleRequest};

    fn db() -> Db {
        crate::test_db().expect("in-memory db")
    }

    fn restock(db: &Db, name: &str, batch_no: &str, qty: i64, price: f64) -> i64 {
        domain::restock(
            db,
            &RestockRequest {
                product: Some(NewProduct {
                    name: name.into(),
                    category: Some("Pharma".into()),
                    unit: Some("pcs".into()),
                    ..NewProduct::default()
                }),
                batch: NewBatch {
                    product_id: 0,
                    batch_no: batch_no.into(),
                    expiry_date: None,
                    unit_price: price,
                    quantity: qty,
                    received_at: None,
                    supplier: None,
                    notes: None,
                },
                source: Some("ui".into()),
                actor: None,
                note: None,
            },
        )
        .unwrap()
        .product_id
    }

    fn sell(db: &Db, pid: i64, qty: i64, buyer: &str) {
        domain::sell(
            db,
            &SaleRequest {
                product_id: pid,
                quantity: qty,
                buyer_name: buyer.into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .unwrap_or_else(|e| panic!("sale failed: {e:?}"));
    }

    fn page(db: &Db, f: &HistoryFilter) -> HistoryPage {
        db.read(|c| history(c, f)).unwrap()
    }

    fn all_rows(db: &Db, f: &HistoryFilter) -> Vec<LedgerEntry> {
        let p = page(db, f);
        p.days
            .into_iter()
            .flat_map(|d| d.entries)
            .collect()
    }

    /// AC-4: newest first, and sale rows carry the buyer.
    #[test]
    fn history_is_newest_first_and_carries_buyer_names() {
        let db = db();
        let pid = restock(&db, "Amox", "A1", 100, 10.0);
        sell(&db, pid, 5, "Mr. Sharma");
        sell(&db, pid, 3, "Carewell Pharmacy");

        let p = page(&db, &HistoryFilter::default());
        assert_eq!(p.total_rows, 3, "one restock plus two sales");

        let rows = all_rows(&db, &HistoryFilter::default());
        assert_eq!(rows[0].buyer_name.as_deref(), Some("Carewell Pharmacy"), "newest sale first");
        assert_eq!(rows[1].buyer_name.as_deref(), Some("Mr. Sharma"));
        assert_eq!(rows[2].buyer_name, None, "restock rows have no buyer");

        // Ledger timestamps are whole seconds, so assert ordering rather than equality.
        let ts: Vec<&str> = rows.iter().map(|r| r.ts.as_str()).collect();
        let mut sorted = ts.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(ts, sorted, "rows must be ordered newest-first");
    }

    /// AC-4: the buyer filter must select exactly that buyer's sales and nothing else.
    #[test]
    fn buyer_filter_selects_only_that_buyer() {
        let db = db();
        let pid = restock(&db, "Panadol", "P1", 100, 8.0);
        sell(&db, pid, 2, "Apollo Franchise");
        sell(&db, pid, 4, "Neha Gupta");

        let filtered = all_rows(
            &db,
            &HistoryFilter {
                buyer_name: Some("neha gupta".into()),
                ..HistoryFilter::default()
            },
        );
        assert_eq!(filtered.len(), 1, "buyer match is case-insensitive");
        assert_eq!(filtered[0].buyer_name.as_deref(), Some("Neha Gupta"));
        assert_eq!(filtered[0].qty_delta, -4);
    }

    /// AC-4: CSV export must match the filtered slice exactly. `history_flat` shares the
    /// predicate set, so a row present in the page must be present in the export and vice versa.
    #[test]
    fn csv_export_matches_the_filtered_slice_exactly() {
        let db = db();
        let pid = restock(&db, "Vitamin D", "D1", 60, 30.0);
        sell(&db, pid, 5, "Buyer One");
        sell(&db, pid, 7, "Buyer Two");

        for f in [
            HistoryFilter::default(),
            HistoryFilter { buyer_name: Some("Buyer One".into()), ..HistoryFilter::default() },
            HistoryFilter { kinds: Some(vec!["sale".into()]), ..HistoryFilter::default() },
            HistoryFilter { kinds: Some(vec!["restock".into()]), ..HistoryFilter::default() },
        ] {
            let shown: Vec<i64> = all_rows(&db, &f).into_iter().map(|r| r.id).collect();
            let exported: Vec<i64> =
                db.read(|c| history_flat(c, &f)).unwrap().into_iter().map(|r| r.id).collect();
            assert_eq!(
                shown, exported,
                "export and page disagree for filter {:?}",
                f.buyer_name
            );
        }
    }

    /// The distinct-buyer list feeds the filter dropdown and must exclude nulls and blanks.
    #[test]
    fn buyer_list_is_distinct_and_non_empty() {
        let db = db();
        let pid = restock(&db, "Surgical Mask", "M1", 100, 5.0);
        sell(&db, pid, 1, "Clinic A");
        sell(&db, pid, 1, "Clinic A");
        sell(&db, pid, 1, "Clinic B");

        let p = page(&db, &HistoryFilter::default());
        assert_eq!(p.buyers, vec!["Clinic A".to_string(), "Clinic B".to_string()]);
        assert!(p.buyers.iter().all(|b| !b.trim().is_empty()));
    }

    /// Per-day subtotals must sum the day and keep in/out value as positive magnitudes.
    #[test]
    fn day_subtotals_are_positive_and_balanced() {
        let db = db();
        let pid = restock(&db, "Electrolyte", "E1", 40, 272.0);
        sell(&db, pid, 4, "Vikram Hospital");
        sell(&db, pid, 6, "Carewell Pharmacy");

        let p = page(&db, &HistoryFilter::default());
        // Everything happened today, so exactly one day bucket.
        assert_eq!(p.days.len(), 1);
        let day = &p.days[0];

        assert_eq!(day.units_in, 40);
        assert_eq!(day.units_out, 10);
        assert_eq!(day.value_in, 40.0 * 272.0);
        assert_eq!(day.value_out, 10.0 * 272.0, "value out is a positive magnitude");

        // The page-level totals must agree with the per-day sums.
        assert_eq!(p.units_in, day.units_in);
        assert_eq!(p.units_out, day.units_out);
        assert_eq!(p.value_out, day.value_out);
    }

    /// A date-range filter must clip on both ends, inclusive.
    #[test]
    fn date_range_filter_clips_inclusively() {
        let db = db();
        let pid = restock(&db, "Dated Item", "T1", 100, 2.0);
        let today = crate::util::today_str();
        let yesterday = crate::util::add_days(crate::util::today(), -1);
        let long_ago = crate::util::add_days(crate::util::today(), -40);

        db.write(|tx| {
            for (ts, buyer) in [(&today, "Today Buyer"), (&yesterday, "Yday Buyer"), (&long_ago, "Old Buyer")] {
                append_ledger(
                    tx,
                    &LedgerRow {
                        id: 0,
                        ts: format!("{ts}T09:00:00Z"),
                        kind: "sale".into(),
                        product_id: pid,
                        product_name: "Dated Item".into(),
                        batch_id: None,
                        batch_no: None,
                        expiry_date: None,
                        qty_delta: -1,
                        unit_price: 2.0,
                        total_value: 2.0,
                        buyer_name: Some(buyer.into()),
                        note: None,
                        source: "ui".into(),
                        actor: None,
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();

        // Scoped to sales: the opening restock is also stamped today, and the point here is the
        // date clip, not the kind filter.
        let sales_only = || HistoryFilter {
            kinds: Some(vec!["sale".into()]),
            ..HistoryFilter::default()
        };

        let two_days = all_rows(
            &db,
            &HistoryFilter {
                from: Some(yesterday.clone().into()),
                to: Some(today.clone().into()),
                ..sales_only()
            },
        );
        assert_eq!(two_days.len(), 2, "yesterday and today, the 40-day-old row excluded");
        assert!(two_days.iter().all(|r| r.buyer_name.as_deref() != Some("Old Buyer")));

        let only_today = all_rows(
            &db,
            &HistoryFilter {
                from: Some(today.into()),
                ..sales_only()
            },
        );
        assert_eq!(only_today.len(), 1);
        assert_eq!(only_today[0].buyer_name.as_deref(), Some("Today Buyer"));

        // The window must clip the upper bound too, not just the lower.
        let only_yesterday = all_rows(
            &db,
            &HistoryFilter {
                to: Some(yesterday.into()),
                ..sales_only()
            },
        );
        assert_eq!(only_yesterday.len(), 2, "yesterday and the 40-day-old row");
        assert!(only_yesterday
            .iter()
            .all(|r| r.buyer_name.as_deref() != Some("Today Buyer")));
    }

    /// An unknown kind string must be ignored, not treated as a filter that matches everything
    /// or nothing silently.
    #[test]
    fn unknown_kind_is_ignored_rather_than_emptying_the_page() {
        let db = db();
        let pid = restock(&db, "Kinded", "K1", 10, 1.0);
        sell(&db, pid, 1, "B");

        let filtered = page(
            &db,
            &HistoryFilter {
                kinds: Some(vec!["not-a-kind".into()]),
                ..HistoryFilter::default()
            },
        );
        assert_eq!(filtered.total_rows, 2, "invalid kind is discarded, not applied");
    }

    /// FTS-backed search must find a product by SKU as well as by name.
    #[test]
    fn search_matches_name_and_sku() {
        let db = db();
        let pid = restock(&db, "Amoxicillin 500mg Capsules", "A9", 10, 12.0);
        db.write(|tx| {
            tx.execute("UPDATE products SET sku = 'AMX-500' WHERE id = ?1", [pid])?;
            Ok(())
        })
        .unwrap();

        db.read(|c| {
            assert!(!search_products(c, "amoxicillin", 8)?.is_empty(), "name search");
            assert!(!search_products(c, "amx-500", 8)?.is_empty(), "SKU search");
            assert!(search_products(c, "zzzznotathing", 8)?.is_empty(), "no false positives");
            Ok(())
        })
        .unwrap();
    }
}
