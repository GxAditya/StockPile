//! Inventory mutations. Every function here runs inside a single IMMEDIATE transaction that
//! writes the stock change **and** the ledger rows together — if the ledger write fails, the
//! stock change rolls back with it. The ledger is the source of truth for history; `batches`
//! is a materialised projection of it.

use crate::db::repo::{self, LedgerRow};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::util;

fn normalise_source(s: Option<&str>) -> String {
    let v = s.unwrap_or("ui").trim().to_lowercase();
    if ["ui", "agent", "seed", "import"].contains(&v.as_str()) {
        v
    } else {
        "ui".into()
    }
}

fn validate_money(v: f64, field: &str) -> AppResult<f64> {
    if !v.is_finite() || v < 0.0 {
        return Err(AppError::validation(format!("{field} must be zero or positive")));
    }
    if v > 1_000_000_000.0 {
        return Err(AppError::validation(format!("{field} value is implausibly large")));
    }
    // Round to 2dp to keep money exact enough for a float-backed ledger.
    Ok((v * 100.0).round() / 100.0)
}

fn validate_expiry(s: &str) -> AppResult<String> {
    let d = util::parse_date(s)
        .ok_or_else(|| AppError::validation(format!("'{s}' is not a valid date (expected YYYY-MM-DD)")))?;
    // A 5-year forward bound catches typos like 20272-01-01 and century mistakes.
    let max = util::today() + chrono::Duration::days(365 * 5);
    let min = util::today() - chrono::Duration::days(365 * 20);
    if d > max {
        return Err(AppError::validation(format!(
            "expiry date {s} is more than 5 years in the future"
        )));
    }
    if d < min {
        return Err(AppError::validation(format!(
            "expiry date {s} is more than 20 years in the past"
        )));
    }
    Ok(d.format("%Y-%m-%d").to_string())
}

// ── Restock ──────────────────────────────────────────────────────────────────────────────

/// Add stock. Creates the product when `req.product` is supplied, otherwise targets an
/// existing product id. Writes one `restock` ledger row alongside the batch.
pub fn restock(db: &Db, req: &RestockRequest) -> AppResult<SaleResult> {
    let batch = &req.batch;
    let batch_no = batch.batch_no.trim().to_string();
    if batch_no.is_empty() {
        return Err(AppError::validation("batch number is required"));
    }
    if batch_no.chars().count() > 64 {
        return Err(AppError::validation("batch number is too long (max 64 characters)"));
    }
    if batch.quantity < 1 {
        return Err(AppError::validation("quantity must be at least 1"));
    }
    if batch.quantity > 10_000_000 {
        return Err(AppError::validation("quantity is implausibly large"));
    }
    let unit_price = validate_money(batch.unit_price, "unit price")?;
    let expiry = match batch.expiry_date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(validate_expiry(s)?),
        None => None,
    };
    let received = match batch.received_at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => validate_expiry(s).map_err(|_| AppError::validation(format!("'{s}' is not a valid received date")))?,
        None => util::today_str(),
    };
    let source = normalise_source(req.source.as_deref());
    let actor = util::clean_opt(req.actor.as_deref());

    let db = db;
    db.write(|tx| {
        let conn = tx;

        // Resolve or create the parent product.
        let (product_id, product_name) = if let Some(np) = req.product.as_ref().filter(|p| !p.name.trim().is_empty()) {
            match repo::find_product_by_name(conn, np.name.trim())? {
                Some(existing) => (existing.id, existing.name),
                None => {
                    let p = repo::create_product(conn, np)?;
                    (p.id, p.name)
                }
            }
        } else {
            let p = repo::find_product(conn, batch.product_id)?;
            (p.id, p.name)
        };

        // Same batch number = top up the existing batch rather than creating a duplicate.
        let existing = repo::find_batch(conn, product_id, &batch_no)?;
        let batch_id = match existing {
            Some(mut b) => {
                let new_qty = b.quantity + batch.quantity;
                if new_qty > 10_000_000 {
                    return Err(AppError::validation("resulting batch quantity is implausibly large"));
                }
                // Weighted-average cost keeps the batch's unit economics honest after a top-up.
                let blended = if new_qty > 0 {
                    ((b.unit_price * b.quantity as f64) + (unit_price * batch.quantity as f64)) / new_qty as f64
                } else {
                    unit_price
                };
                conn.execute(
                    "UPDATE batches SET quantity = ?1, unit_price = ?2, expiry_date = COALESCE(?3, expiry_date),
                            supplier = COALESCE(?4, supplier), notes = COALESCE(?5, notes)
                     WHERE id = ?6",
                    rusqlite::params![
                        new_qty,
                        (blended * 100.0).round() / 100.0,
                        expiry.clone(),
                        util::clean_opt(batch.supplier.as_deref()),
                        util::clean_opt(batch.notes.as_deref()),
                        b.id
                    ],
                )?;
                b.quantity = new_qty;
                b.id
            }
            None => {
                conn.execute(
                    "INSERT INTO batches (product_id, batch_no, expiry_date, unit_price, quantity,
                                          received_at, supplier, notes)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![
                        product_id,
                        batch_no,
                        expiry.clone(),
                        unit_price,
                        batch.quantity,
                        received,
                        util::clean_opt(batch.supplier.as_deref()),
                        util::clean_opt(batch.notes.as_deref())
                    ],
                )?;
                conn.last_insert_rowid()
            }
        };

        let now = util::now_ts();
        let ledger_id = repo::append_ledger(
            conn,
            &LedgerRow {
                id: 0,
                ts: now.clone(),
                kind: LedgerKind::Restock.as_str().into(),
                product_id,
                product_name: product_name.clone(),
                batch_id: Some(batch_id),
                batch_no: Some(batch_no.clone()),
                expiry_date: expiry.clone(),
                qty_delta: batch.quantity,
                unit_price,
                total_value: (batch.quantity as f64 * unit_price * 100.0).round() / 100.0,
                buyer_name: None,
                note: util::clean_opt(req.note.as_deref()),
                source: source.clone(),
                actor: actor.clone(),
            },
        )?;

        repo::bump_usage_day(conn, product_id, &received, 0, batch.quantity)?;

        let remaining: i64 = conn.query_row(
            "SELECT COALESCE(SUM(quantity),0) FROM batches WHERE product_id = ?1",
            [product_id],
            |r| r.get(0),
        )?;

        Ok(SaleResult {
            product_id,
            product_name,
            quantity: batch.quantity,
            total_value: (batch.quantity as f64 * unit_price * 100.0).round() / 100.0,
            allocations: vec![SaleAllocation {
                batch_id,
                batch_no,
                expiry_date: expiry,
                unit_price,
                quantity: batch.quantity,
            }],
            remaining_quantity: remaining,
            ledger_ids: vec![ledger_id],
        })
    })
}

// ── FEFO ────────────────────────────────────────────────────────────────────────────────

/// First Expiry, First Out allocation. Earliest expiry drains first; batches with no expiry
/// sort last; expired batches sort after every usable batch so they are only drawn when the
/// caller explicitly allows it.
pub struct AllocationPlan {
    pub allocations: Vec<SaleAllocation>,
    pub shortfall: i64,
}

pub fn plan_allocation(
    db: &Db,
    product_id: i64,
    quantity: i64,
    allow_expired: bool,
) -> AppResult<AllocationPlan> {
    let db = db;
    db.read(|conn| {
        let today = util::today_str();
        let candidates = repo::live_batches_fefo(conn, product_id)?;
        let mut allocations = Vec::new();
        let mut remaining = quantity;

        for b in candidates {
            if remaining <= 0 {
                break;
            }
            let expired = b
                .expiry_date
                .as_deref()
                .map(|d| d < today.as_str())
                .unwrap_or(false);
            if expired && !allow_expired {
                continue;
            }
            let take = remaining.min(b.quantity);
            allocations.push(SaleAllocation {
                batch_id: b.id,
                batch_no: b.batch_no,
                expiry_date: b.expiry_date,
                unit_price: b.unit_price,
                quantity: take,
            });
            remaining -= take;
        }

        Ok(AllocationPlan {
            shortfall: remaining.max(0),
            allocations,
        })
    })
}

/// Sell stock, draining batches FEFO. One ledger row per batch consumed, so every sold unit
/// keeps its batch provenance. Rejects shortfalls without touching the database.
pub fn sell(db: &Db, req: &SaleRequest) -> AppResult<SaleResult> {
    if req.quantity < 1 {
        return Err(AppError::validation("sale quantity must be at least 1"));
    }
    let buyer = req
        .buyer_name
        .trim()
        .replace(char::is_whitespace, " ")
        .trim()
        .to_string();
    if buyer.is_empty() {
        return Err(AppError::validation("buyer name is required"));
    }
    if buyer.chars().count() > 120 {
        return Err(AppError::validation("buyer name is too long (max 120 characters)"));
    }
    let override_price = match req.unit_price_override {
        Some(p) => Some(validate_money(p, "unit price")?),
        None => None,
    };
    let source = normalise_source(req.source.as_deref());
    let actor = util::clean_opt(req.actor.as_deref());
    let note = util::clean_opt(req.note.as_deref());

    // Plan outside the write transaction so a shortfall never takes the writer lock.
    let plan = plan_allocation(db, req.product_id, req.quantity, req.allow_expired)?;
    if plan.shortfall > 0 {
        let available: i64 = plan.allocations.iter().map(|a| a.quantity).sum();
        let name = db.read(|c| Ok(repo::find_product(c, req.product_id).map(|p| p.name)))?
            .unwrap_or_else(|_| format!("product {}", req.product_id));
        return Err(AppError::Shortfall {
            product: name,
            requested: req.quantity,
            available,
        });
    }
    if plan.allocations.is_empty() {
        let name = db.read(|c| Ok(repo::find_product(c, req.product_id).map(|p| p.name)))?
            .unwrap_or_else(|_| format!("product {}", req.product_id));
        return Err(AppError::Shortfall {
            product: name,
            requested: req.quantity,
            available: 0,
        });
    }

    let db = db;
    db.write(|tx| {
        let conn = tx;
        let product = repo::find_product(conn, req.product_id)?;
        let today = util::today_str();

        for a in &plan.allocations {
            // Conditional decrement: the `quantity >= ?` guard makes an oversell impossible even
            // if another writer slipped a change in between plan and commit.
            let changed = conn.execute(
                "UPDATE batches SET quantity = quantity - ?1 WHERE id = ?2 AND quantity >= ?1",
                rusqlite::params![a.quantity, a.batch_id],
            )?;
            if changed != 1 {
                return Err(AppError::Conflict(format!(
                    "batch {} changed while the sale was being applied",
                    a.batch_no
                )));
            }
        }

        let mut total = 0.0;
        let mut ledger_ids = Vec::new();
        for a in &plan.allocations {
            let price = override_price.unwrap_or(a.unit_price);
            let line = (a.quantity as f64 * price * 100.0).round() / 100.0;
            total += line;
            let ledger_id = repo::append_ledger(
                conn,
                &LedgerRow {
                    id: 0,
                    ts: util::now_ts(),
                    kind: LedgerKind::Sale.as_str().into(),
                    product_id: product.id,
                    product_name: product.name.clone(),
                    batch_id: Some(a.batch_id),
                    batch_no: Some(a.batch_no.clone()),
                    expiry_date: a.expiry_date.clone(),
                    qty_delta: -a.quantity,
                    unit_price: price,
                    total_value: line,
                    buyer_name: Some(buyer.clone()),
                    note: note.clone(),
                    source: source.clone(),
                    actor: actor.clone(),
                },
            )?;
            ledger_ids.push(ledger_id);
        }

        repo::bump_usage_day(conn, product.id, &today, req.quantity, 0)?;

        let remaining: i64 = conn.query_row(
            "SELECT COALESCE(SUM(quantity),0) FROM batches WHERE product_id = ?1",
            [product.id],
            |r| r.get(0),
        )?;

        Ok(SaleResult {
            product_id: product.id,
            product_name: product.name,
            quantity: req.quantity,
            total_value: (total * 100.0).round() / 100.0,
            allocations: plan.allocations.clone(),
            remaining_quantity: remaining,
            ledger_ids,
        })
    })
}

// ── Adjustment ──────────────────────────────────────────────────────────────────────────

/// Manual correction for stock that drifted (damage, miscount, write-off). Recorded as an
/// `adjustment` so it is visibly distinct from a purchase or a sale.
pub fn adjust(db: &Db, req: &AdjustRequest) -> AppResult<SaleResult> {
    if req.quantity_delta == 0 {
        return Err(AppError::validation("adjustment quantity cannot be zero"));
    }
    let reason = req
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::validation("a reason is required for a stock adjustment"))?;
    let source = normalise_source(req.source.as_deref());
    let actor = util::clean_opt(req.actor.as_deref());

    let db = db;
    db.write(|tx| {
        let conn = tx;
        let product = repo::find_product(conn, req.product_id)?;

        let batch_id = match req.batch_no.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(no) => match repo::find_batch(conn, product.id, no)? {
                Some(b) => {
                    let new_qty = b.quantity + req.quantity_delta;
                    if new_qty < 0 {
                        return Err(AppError::Shortfall {
                            product: product.name.clone(),
                            requested: -req.quantity_delta,
                            available: b.quantity,
                        });
                    }
                    conn.execute(
                        "UPDATE batches SET quantity = ?1 WHERE id = ?2",
                        rusqlite::params![new_qty, b.id],
                    )?;
                    Some(b.id)
                }
                None => {
                    return Err(AppError::NotFound(format!(
                        "batch '{no}' does not exist for {}",
                        product.name
                    )))
                }
            },
            None => {
                // No batch specified: apply against the newest non-empty batch, or open one.
                let existing: Option<(i64, String)> = conn
                    .query_row(
                        "SELECT id, batch_no FROM batches
                         WHERE product_id = ?1 AND quantity > 0
                         ORDER BY received_at DESC, id DESC LIMIT 1",
                        [product.id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .ok();
                match existing {
                    Some((id, _)) => {
                        let current: i64 = conn
                            .query_row("SELECT quantity FROM batches WHERE id = ?1", [id], |r| r.get(0))?;
                        let new_qty = current + req.quantity_delta;
                        if new_qty < 0 {
                            return Err(AppError::Shortfall {
                                product: product.name.clone(),
                                requested: -req.quantity_delta,
                                available: current,
                            });
                        }
                        conn.execute(
                            "UPDATE batches SET quantity = ?1 WHERE id = ?2",
                            rusqlite::params![new_qty, id],
                        )?;
                        Some(id)
                    }
                    None => {
                        if req.quantity_delta < 0 {
                            return Err(AppError::Shortfall {
                                product: product.name,
                                requested: -req.quantity_delta,
                                available: 0,
                            });
                        }
                        conn.execute(
                            "INSERT INTO batches (product_id, batch_no, expiry_date, unit_price,
                                                  quantity, received_at, supplier, notes)
                             VALUES (?1, 'ADJUSTED', NULL, 0, ?2, ?3, NULL, ?4)",
                            rusqlite::params![
                                product.id,
                                req.quantity_delta,
                                util::today_str(),
                                format!("Opened by adjustment: {reason}")
                            ],
                        )?;
                        Some(conn.last_insert_rowid())
                    }
                }
            }
        };

        let (price, expiry, batch_no) = match batch_id {
            Some(id) => conn.query_row(
                "SELECT unit_price, expiry_date, batch_no FROM batches WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, f64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?)),
            )?,
            None => (0.0, None, "ADJUSTED".to_string()),
        };

        let ledger_id = repo::append_ledger(
            conn,
            &LedgerRow {
                id: 0,
                ts: util::now_ts(),
                kind: LedgerKind::Adjustment.as_str().into(),
                product_id: product.id,
                product_name: product.name.clone(),
                batch_id,
                batch_no: Some(batch_no.clone()),
                expiry_date: expiry.clone(),
                qty_delta: req.quantity_delta,
                unit_price: price,
                total_value: (req.quantity_delta.abs() as f64 * price * 100.0).round() / 100.0
                    * if req.quantity_delta < 0 { -1.0 } else { 1.0 },
                buyer_name: None,
                note: Some(reason.to_string()),
                source: source.clone(),
                actor: actor.clone(),
            },
        )?;

        let remaining: i64 = conn.query_row(
            "SELECT COALESCE(SUM(quantity),0) FROM batches WHERE product_id = ?1",
            [product.id],
            |r| r.get(0),
        )?;

        Ok(SaleResult {
            product_id: product.id,
            product_name: product.name,
            quantity: req.quantity_delta,
            total_value: (req.quantity_delta.abs() as f64 * price).round() / 100.0,
            allocations: vec![SaleAllocation {
                batch_id: batch_id.unwrap_or(0),
                batch_no,
                expiry_date: expiry,
                unit_price: price,
                quantity: req.quantity_delta,
            }],
            remaining_quantity: remaining,
            ledger_ids: vec![ledger_id],
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analytics, db::repo, domain};

    fn db() -> Db {
        crate::test_db().expect("in-memory db")
    }

    fn restock(db: &Db, name: &str, batch_no: &str, qty: i64, price: f64, expiry: &str) -> i64 {
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
                    expiry_date: Some(expiry.into()),
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
        .unwrap_or_else(|e| panic!("restock failed: {e:?}"))
        .product_id
    }

    fn on_hand(db: &Db, product_id: i64) -> i64 {
        db.read(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(quantity),0) FROM batches WHERE product_id = ?1",
                [product_id],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap()
    }

    fn batch_qty(db: &Db, product_id: i64, batch_no: &str) -> i64 {
        db.read(|c| {
            Ok(c.query_row(
                "SELECT quantity FROM batches WHERE product_id = ?1 AND batch_no = ?2",
                rusqlite::params![product_id, batch_no],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap()
    }

    fn ledger_rows(db: &Db, kind: &str) -> Vec<(i64, i64, f64, Option<String>)> {
        db.read(|c| {
            let mut st = c
                .prepare("SELECT qty_delta, batch_id, total_value, buyer_name FROM ledger WHERE kind = ?1 ORDER BY id")
                .unwrap();
            let v = st
                .query_map([kind], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, Option<String>>(3)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            Ok(v)
        })
        .unwrap()
    }

    /// AC-1: restocking 100 units at 12.50 makes stock 100 and writes exactly one ledger row
    /// with qty_delta = +100.
    #[test]
    fn ac1_restock_writes_one_positive_ledger_row() {
        let db = db();
        let pid = restock(&db, "Amoxicillin 500mg", "B1", 100, 12.50, "2027-03-01");

        assert_eq!(on_hand(&db, pid), 100);

        let rows = ledger_rows(&db, "restock");
        assert_eq!(rows.len(), 1, "exactly one restock ledger row");
        assert_eq!(rows[0].0, 100, "qty_delta must be +100");
        assert_eq!(rows[0].2, 1250.00, "100 x 12.50");
    }

    /// AC-2: FEFO across two batches — 3 from the earliest-expiring, 2 from the next, producing
    /// one sale ledger row per batch consumed, both carrying the buyer name.
    #[test]
    fn ac2_fefo_drains_earliest_expiry_first() {
        let db = db();
        let pid = restock(&db, "Panadol 500mg", "LATE", 50, 10.0, "2028-01-01");
        restock(&db, "Panadol 500mg", "EARLY", 3, 10.0, "2026-11-01");

        let out = domain::sell(
            &db,
            &SaleRequest {
                product_id: pid,
                quantity: 5,
                buyer_name: "Mr. Sharma".into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .expect("sale should succeed");

        assert_eq!(out.allocations.len(), 2, "one allocation per batch consumed");
        assert_eq!(out.allocations[0].batch_no, "EARLY", "earliest expiry drawn first");
        assert_eq!(out.allocations[0].quantity, 3);
        assert_eq!(out.allocations[1].batch_no, "LATE");
        assert_eq!(out.allocations[1].quantity, 2);

        assert_eq!(batch_qty(&db, pid, "EARLY"), 0, "early batch drained");
        assert_eq!(batch_qty(&db, pid, "LATE"), 48);
        assert_eq!(on_hand(&db, pid), 48);

        let sales = ledger_rows(&db, "sale");
        assert_eq!(sales.len(), 2, "one sale ledger row per batch consumed");
        for (_, _, _, buyer) in &sales {
            assert_eq!(buyer.as_deref(), Some("Mr. Sharma"), "sale rows carry buyer");
        }
        assert!(sales.iter().all(|(d, _, _, _)| *d < 0), "sale qty_delta is negative");
    }

    /// AC-3: a shortfall is rejected with the gap named, and nothing is written.
    #[test]
    fn ac3_shortfall_rejected_and_db_unchanged() {
        let db = db();
        let pid = restock(&db, "Electrolyte Powder", "E1", 12, 272.0, "2027-06-30");

        let err = domain::sell(
            &db,
            &SaleRequest {
                product_id: pid,
                quantity: 40,
                buyer_name: "Carewell Pharmacy".into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .expect_err("selling 40 with 12 on hand must fail");

        match err {
            AppError::Shortfall { requested, available, .. } => {
                assert_eq!(requested, 40);
                assert_eq!(available, 12);
            }
            other => panic!("expected Shortfall, got {other:?}"),
        }

        assert_eq!(on_hand(&db, pid), 12, "stock untouched");
        assert!(ledger_rows(&db, "sale").is_empty(), "no ledger row written");
    }

    /// `total_value` is a magnitude in the ledger; direction lives in `qty_delta`. Revenue must
    /// therefore come back positive for a pure-sales ledger.
    #[test]
    fn ledger_total_value_is_a_magnitude_not_signed() {
        let db = db();
        let pid = restock(&db, "Vitamin C", "V1", 20, 5.0, "2027-01-01");
        domain::sell(
            &db,
            &SaleRequest {
                product_id: pid,
                quantity: 4,
                buyer_name: "Apollo".into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .unwrap();

        let sales = ledger_rows(&db, "sale");
        assert_eq!(sales.len(), 1);
        assert_eq!(sales[0].2, 20.0, "4 x 5.00 stored as a positive magnitude");

        let dash = analytics::dashboard(&db).expect("dashboard");
        assert_eq!(dash.kpis.revenue_30d, 20.0, "revenue is positive");
        assert_eq!(dash.kpis.units_sold_30d, 4);
        assert_eq!(dash.top_sold_30d.len(), 1, "top sellers is populated for recent sales");
        assert_eq!(dash.top_sold_30d[0].value, 20.0, "top-seller value is positive");
    }

    /// Expired batches sort last and are only drawn when explicitly allowed.
    #[test]
    fn expired_batches_require_the_override() {
        let db = db();
        let pid = restock(&db, "Old Stock", "EXP", 10, 2.0, "2020-01-01");
        restock(&db, "Old Stock", "FRESH", 4, 2.0, "2028-01-01");

        let plan = domain::plan_allocation(&db, pid, 6, false).unwrap();
        assert!(plan.allocations.iter().all(|a| a.batch_no == "FRESH"),
                "expired batch is skipped without the override");
        assert_eq!(plan.shortfall, 2, "the expired units are reported as a shortfall");

        let plan = domain::plan_allocation(&db, pid, 6, true).unwrap();
        assert_eq!(plan.allocations.len(), 2, "override permits the expired batch");
        assert_eq!(plan.shortfall, 0);
    }

    /// Restocking a repeated batch number tops it up and blends the unit cost, rather than
    /// creating a duplicate batch.
    #[test]
    fn repeated_batch_number_blends_cost() {
        let db = db();
        restock(&db, "Paracetamol", "P1", 10, 10.0, "2027-05-01");
        let pid = restock(&db, "Paracetamol", "P1", 10, 20.0, "2027-05-01");

        assert_eq!(on_hand(&db, pid), 20, "topped up, not duplicated");
        let (qty, price): (i64, f64) = db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT quantity, unit_price FROM batches WHERE product_id = ?1 AND batch_no = 'P1'",
                    [pid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(qty, 20);
        assert_eq!(price, 15.0, "weighted-average blend of 10@10 and 10@20");
    }

    /// `stock_list` is read by name, so a projection edit cannot silently shift a column into
    /// the wrong field. This exercises every mapped column against a real multi-batch row.
    #[test]
    fn stock_list_maps_every_column_correctly() {
        let db = db();
        let pid = restock(&db, "Amoxicillin 500mg", "A-EARLY", 3, 12.75, "2026-11-01");
        restock(&db, "Amoxicillin 500mg", "A-LATE", 40, 15.00, "2028-01-01");

        let list = db
            .read(|c| {
                repo::stock_list(
                    c,
                    &repo::StockQuery {
                        search: None,
                        category: None,
                        health: None,
                        sort: Some("name".into()),
                        limit: Some(100),
                        offset: None,
                    },
                )
            })
            .expect("stock_list must not fail");

        assert_eq!(list.rows.len(), 1);
        let r = &list.rows[0];
        assert_eq!(r.id, pid);
        assert_eq!(r.name, "Amoxicillin 500mg");
        assert_eq!(r.category.as_deref(), Some("Pharma"));
        assert_eq!(r.unit, "pcs");
        assert_eq!(r.total_quantity, 43, "3 + 40");
        assert_eq!(r.batch_count, 2);
        assert_eq!(r.earliest_expiry.as_deref(), Some("2026-11-01"));
        assert_eq!(r.expiring_units, 3, "the November batch is inside 30 days");
        assert_eq!(r.expired_units, 0);
        // total_value = 3*12.75 + 40*15.00 = 638.25; avg_cost = 638.25 / 43
        assert!((r.total_value - 638.25).abs() < 0.005, "total_value {}", r.total_value);
        assert!((r.avg_cost - 638.25 / 43.0).abs() < 0.005, "avg_cost {}", r.avg_cost);
        assert!(r.last_movement_at.is_some(), "ledger movement is projected through");
        assert!(matches!(r.health, StockHealth::Expiring), "health {:?}", r.health);
    }
}
