//! Deterministic inventory heuristics. Every card produced here states the rule that
//! generated it — a heuristic nobody can audit is a heuristic nobody trusts. No model is
//! involved in this module at all, which is also why it costs zero RAM.

use crate::db::Db;
use crate::error::AppResult;
use crate::util;
use serde::Serialize;

const DEAD_STOCK_DAYS: i64 = 90;
const USAGE_WINDOW_DAYS: i64 = 30;
const SAFETY_FACTOR: f64 = 1.5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: String,
    pub title: String,
    pub rule: String,
    pub severity: String,
    pub detail: String,
    pub metric: Option<f64>,
    pub metric_label: Option<String>,
    pub items: Vec<InsightItem>,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightItem {
    pub product_id: i64,
    pub product_name: String,
    pub batch_no: Option<String>,
    pub quantity: Option<i64>,
    pub value: Option<f64>,
    pub days: Option<i64>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub generated_at: String,
    pub total_flagged: usize,
    pub insights: Vec<Insight>,
}

fn money(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn all(db: &Db) -> AppResult<Insights> {
    let today = util::today();
    let today_s = util::today_str();
    let window_start = util::add_days(today, -(USAGE_WINDOW_DAYS - 1));
    let mut insights = Vec::new();

    db.read(|conn| {
        // ── 1. Expiry risk ────────────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.name, b.batch_no, b.quantity, b.quantity*b.unit_price,
                        julianday(b.expiry_date) - julianday(?1)
                 FROM batches b JOIN products p ON p.id = b.product_id
                 WHERE b.quantity > 0 AND b.expiry_date IS NOT NULL
                   AND b.expiry_date <= ?2
                 ORDER BY b.expiry_date ASC LIMIT 50",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map(rusqlite::params![today_s.clone(), util::add_days(today, 30)], |r| {
                    let days = r.get::<_, f64>(5)?.floor() as i64;
                    let qty: i64 = r.get(3)?;
                    Ok(InsightItem {
                        product_id: r.get(0)?,
                        product_name: r.get(1)?,
                        batch_no: Some(r.get(2)?),
                        quantity: Some(qty),
                        value: Some(money(r.get(4)?)),
                        days: Some(days),
                        suggestion: Some(match days {
                            d if d < 0 => "Write off or quarantine — regulatory risk".into(),
                            0..=7 => "Urgent: discount heavily or return to supplier".into(),
                            8..=14 => "Clear now: markdown or bundle".into(),
                            _ => "Plan clearance before it becomes a write-off".into(),
                        }),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let at_risk: f64 = items.iter().filter_map(|i| i.value).sum();
            insights.push(Insight {
                id: "expiry_risk".into(),
                title: "Expiry risk".into(),
                rule: "Live batches whose expiry date is today or earlier than today + 30 days.".into(),
                severity: if items.iter().any(|i| i.days.unwrap_or(0) < 0) { "critical" } else { "warning" }.into(),
                detail: format!(
                    "{} batch(es) need a decision, holding {} of stock value at risk.",
                    items.len(),
                    money(at_risk)
                ),
                metric: Some(money(at_risk)),
                metric_label: Some("value at risk".into()),
                item_count: items.len(),
                items,
            });
        }

        // ── 2. Dead stock ──────────────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.name, COALESCE(SUM(b.quantity),0) AS q,
                        COALESCE(SUM(b.quantity*b.unit_price),0) AS v,
                        CAST(julianday(?1) - julianday(MAX(m.last_ts)) AS INTEGER) AS idle
                 FROM products p
                 LEFT JOIN batches b ON b.product_id = p.id
                 LEFT JOIN (SELECT product_id, MAX(ts) AS last_ts FROM ledger GROUP BY product_id) m
                        ON m.product_id = p.id
                 GROUP BY p.id
                 HAVING q > 0 AND (m.last_ts IS NULL OR idle >= ?2)
                 ORDER BY v DESC LIMIT 50",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map(rusqlite::params![today_s.clone(), DEAD_STOCK_DAYS], |r| {
                    let idle: Option<i64> = r.get(4)?;
                    Ok(InsightItem {
                        product_id: r.get(0)?,
                        product_name: r.get(1)?,
                        batch_no: None,
                        quantity: Some(r.get(2)?),
                        value: Some(money(r.get(3)?)),
                        days: idle,
                        suggestion: Some(if idle.is_none() {
                            "Never transacted — review whether it belongs in the catalogue".into()
                        } else {
                            "Consolidate, discount, or return to supplier to free cash".into()
                        }),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let tied: f64 = items.iter().filter_map(|i| i.value).sum();
            insights.push(Insight {
                id: "dead_stock".into(),
                title: "Dead stock".into(),
                rule: format!(
                    "Products with stock on hand and no ledger movement in {DEAD_STOCK_DAYS}+ days."
                ),
                severity: "info".into(),
                detail: format!(
                    "{} product(s) are not moving, tying up {} of capital.",
                    items.len(),
                    money(tied)
                ),
                metric: Some(money(tied)),
                metric_label: Some("capital tied up".into()),
                item_count: items.len(),
                items,
            });
        }

        // ── 3. Reorder suggestions ─────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "WITH usage AS (
                     SELECT product_id, SUM(units_out) AS out30
                     FROM usage_daily WHERE day >= ?1 GROUP BY product_id
                 ),
                 stock AS (
                     SELECT p.id, p.name, p.reorder_point, p.lead_time_days,
                            COALESCE(SUM(b.quantity),0) AS on_hand
                     FROM products p LEFT JOIN batches b ON b.product_id = p.id
                     GROUP BY p.id
                 )
                 SELECT s.id, s.name, s.on_hand,
                        COALESCE(u.out30,0) AS out30,
                        MAX(1.0, s.lead_time_days) AS lead,
                        MAX(1.0, s.reorder_point) AS rp
                 FROM stock s LEFT JOIN usage u ON u.product_id = s.id
                 WHERE COALESCE(u.out30,0) > 0 OR s.reorder_point > 0",
            )?;
            let mut rows: Vec<(i64, String, i64, i64, f64, f64)> = stmt
                .query_map([&window_start], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut items = Vec::new();
            for (id, name, on_hand, out30, lead, rp) in rows.drain(..) {
                let avg_daily = out30 as f64 / USAGE_WINDOW_DAYS as f64;
                if avg_daily <= 0.0 {
                    // No observed demand: fall back to the operator's own reorder point.
                    if (on_hand as f64) > rp {
                        continue;
                    }
                    items.push(InsightItem {
                        product_id: id,
                        product_name: name,
                        batch_no: None,
                        quantity: Some((rp * 2.0 - on_hand as f64).ceil().max(1.0) as i64),
                        value: None,
                        days: None,
                        suggestion: Some(format!(
                            "On hand {on_hand} is at or below your reorder point of {rp:.0}. No sales in the last {USAGE_WINDOW_DAYS} days."
                        )),
                    });
                    continue;
                }
                let target = (avg_daily * lead * SAFETY_FACTOR).ceil() + rp;
                let cover_days = if avg_daily > 0.0 {
                    on_hand as f64 / avg_daily
                } else {
                    f64::INFINITY
                };
                if (on_hand as f64) >= target {
                    continue;
                }
                let order_qty = (target - on_hand as f64).ceil().max(1.0) as i64;
                items.push(InsightItem {
                    product_id: id,
                    product_name: name,
                    batch_no: None,
                    quantity: Some(order_qty),
                    value: None,
                    days: Some(cover_days.floor() as i64),
                    suggestion: Some(format!(
                        "Order ~{} unit(s). Uses {:.1}/day, {lead:.0}-day lead, cover is {cover_days:.1} days.",
                        order_qty, avg_daily
                    )),
                });
            }
            items.sort_by(|a, b| b.quantity.unwrap_or(0).cmp(&a.quantity.unwrap_or(0)));

            insights.push(Insight {
                id: "reorder".into(),
                title: "Reorder suggestions".into(),
                rule: format!(
                    "target = avg_daily_usage({USAGE_WINDOW_DAYS}d) x lead_time x {SAFETY_FACTOR} + reorder_point; flag when on hand < target."
                ),
                severity: "warning".into(),
                detail: format!("{} product(s) projected to run short.", items.len()),
                metric: None,
                metric_label: None,
                item_count: items.len(),
                items: items.into_iter().take(50).collect(),
            });
        }

        // ── 4. Concentration risk ──────────────────────────────────────────────────
        {
            let total: f64 = conn.query_row(
                "SELECT COALESCE(SUM(quantity*unit_price),0) FROM batches WHERE quantity > 0",
                [],
                |r| r.get(0),
            )?;
            let mut stmt = conn.prepare(
                "SELECT COALESCE(NULLIF(p.category,''),'Uncategorised') AS cat,
                        COALESCE(SUM(b.quantity*b.unit_price),0) AS v
                 FROM batches b JOIN products p ON p.id = b.product_id
                 WHERE b.quantity > 0 GROUP BY cat HAVING v > 0 ORDER BY v DESC",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map([], |r| {
                    let v: f64 = r.get(1)?;
                    let share = if total > 0.0 { v / total } else { 0.0 };
                    Ok(InsightItem {
                        product_id: 0,
                        product_name: r.get::<_, String>(0)?,
                        batch_no: None,
                        quantity: None,
                        value: Some(money(v)),
                        days: None,
                        suggestion: Some(format!(
                            "{:.0}% of total inventory value is in this category.",
                            share * 100.0
                        )),
                    })
                })?
                .filter_map(|r: rusqlite::Result<InsightItem>| r.ok())
                .filter(|i| i.value.unwrap_or(0.0) / total.max(1.0) > 0.40)
                .collect();

            insights.push(Insight {
                id: "concentration".into(),
                title: "Concentration risk".into(),
                rule: "Any single category holding more than 40% of total inventory value.".into(),
                severity: "info".into(),
                detail: format!(
                    "{} category/categories exceed the 40% concentration threshold.",
                    items.len()
                ),
                metric: None,
                metric_label: None,
                item_count: items.len(),
                items,
            });
        }

        // ── 5. Price drift ─────────────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "WITH avgp AS (
                     SELECT product_id, SUM(quantity*unit_price)/NULLIF(SUM(quantity),0) AS avg_price
                     FROM batches WHERE quantity > 0 GROUP BY product_id
                 )
                 SELECT b.product_id, p.name, b.batch_no, b.unit_price, a.avg_price, b.quantity
                 FROM batches b
                 JOIN avgp a ON a.product_id = b.product_id
                 JOIN products p ON p.id = b.product_id
                 WHERE b.quantity > 0 AND a.avg_price > 0
                   AND ABS(b.unit_price - a.avg_price) / a.avg_price > 0.15
                 ORDER BY ABS(b.unit_price - a.avg_price)/a.avg_price DESC LIMIT 50",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map([], |r| {
                    let price: f64 = r.get(3)?;
                    let avg: f64 = r.get(4)?;
                    let drift = (price - avg) / avg;
                    Ok(InsightItem {
                        product_id: r.get(0)?,
                        product_name: r.get(1)?,
                        batch_no: Some(r.get(2)?),
                        quantity: Some(r.get(5)?),
                        value: Some(money(price * r.get::<_, i64>(5)? as f64)),
                        days: None,
                        suggestion: Some(format!(
                            "Batch price {price:.2} is {:+.1}% vs weighted average {avg:.2}.",
                            drift * 100.0
                        )),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            insights.push(Insight {
                id: "price_drift".into(),
                title: "Price drift".into(),
                rule: "Batch unit price deviating more than 15% from the product's weighted average cost.".into(),
                severity: "info".into(),
                detail: format!("{} batch(es) have an outlying cost basis.", items.len()),
                metric: None,
                metric_label: None,
                item_count: items.len(),
                items,
            });
        }

        // ── 6. Days of cover ───────────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "WITH usage AS (
                     SELECT product_id, SUM(units_out) AS out30 FROM usage_daily
                     WHERE day >= ?1 GROUP BY product_id
                 ),
                 stock AS (
                     SELECT p.id, p.name, COALESCE(SUM(b.quantity),0) AS on_hand
                     FROM products p LEFT JOIN batches b ON b.product_id = p.id GROUP BY p.id
                 )
                 SELECT s.id, s.name, s.on_hand, COALESCE(u.out30,0)
                 FROM stock s LEFT JOIN usage u ON u.product_id = s.id
                 WHERE COALESCE(u.out30,0) > 0 AND s.on_hand > 0",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map([&window_start], |r| {
                    let on_hand: i64 = r.get(2)?;
                    let out30: i64 = r.get(3)?;
                    let avg = out30 as f64 / USAGE_WINDOW_DAYS as f64;
                    let cover = on_hand as f64 / avg;
                    Ok(InsightItem {
                        product_id: r.get(0)?,
                        product_name: r.get(1)?,
                        batch_no: None,
                        quantity: Some(on_hand),
                        value: None,
                        days: Some(cover.floor() as i64),
                        suggestion: Some(format!(
                            "{cover:.1} days of cover left at {:.1} units/day.",
                            avg
                        )),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter(|i| i.days.unwrap_or(99) <= 14)
                .collect();

            insights.push(Insight {
                id: "days_of_cover".into(),
                title: "Running low on cover".into(),
                rule: "on_hand / avg_daily_usage(30d); flag products with 14 days of cover or fewer.".into(),
                severity: "warning".into(),
                detail: format!("{} product(s) have two weeks of cover or less.", items.len()),
                metric: None,
                metric_label: None,
                item_count: items.len(),
                items,
            });
        }

        // ── 7. Buyer leaderboard ───────────────────────────────────────────────────
        {
            let mut stmt = conn.prepare(
                "SELECT buyer_name, SUM(-qty_delta) AS units, SUM(total_value) AS value,
                        MAX(substr(ts,1,10)) AS last_seen, COUNT(*) AS orders
                 FROM ledger
                 WHERE kind='sale' AND buyer_name IS NOT NULL AND buyer_name <> ''
                 GROUP BY buyer_name ORDER BY value DESC LIMIT 20",
            )?;
            let items: Vec<InsightItem> = stmt
                .query_map([], |r| {
                    Ok(InsightItem {
                        product_id: 0,
                        product_name: r.get::<_, String>(0)?,
                        batch_no: None,
                        quantity: Some(r.get(1)?),
                        value: Some(money(r.get(2)?)),
                        days: Some(util::days_since_ts(&r.get::<_, String>(3)?)),
                        suggestion: Some(format!(
                            "{} order(s), last purchase {} days ago.",
                            r.get::<_, i64>(4)?,
                            util::days_since_ts(&r.get::<_, String>(3)?)
                        )),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            insights.push(Insight {
                id: "buyers".into(),
                title: "Buyer leaderboard".into(),
                rule: "All sale records grouped by buyer name, ranked by total value.".into(),
                severity: "info".into(),
                detail: format!("{} buyer(s) on record.", items.len()),
                metric: None,
                metric_label: None,
                item_count: items.len(),
                items,
            });
        }

        // ── 8. Sales volatility ────────────────────────────────────────────────────
        {
            let start = util::add_days(today, -29);
            let mut stmt = conn.prepare(
                "WITH days(day) AS (
                     SELECT ?1 UNION SELECT date(day,'+1 day') FROM days WHERE day < ?2 LIMIT 30
                 )
                 SELECT days.day, COALESCE((
                     SELECT SUM(-qty_delta) FROM ledger
                     WHERE kind='sale' AND substr(ts,1,10)=days.day), 0)
                 FROM days ORDER BY days.day",
            )?;
            let series: Vec<(String, i64)> = stmt
                .query_map(rusqlite::params![start.clone(), today_s.clone()], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let n = series.len() as f64;
            let mean = series.iter().map(|(_, v)| *v as f64).sum::<f64>() / n.max(1.0);
            let variance = series
                .iter()
                .map(|(_, v)| {
                    let d = *v as f64 - mean;
                    d * d
                })
                .sum::<f64>()
                / n.max(1.0);
            let sd = variance.sqrt();

            let items: Vec<InsightItem> = if sd < 0.001 {
                Vec::new()
            } else {
                series
                    .iter()
                    .filter_map(|(day, v)| {
                        let z = (*v as f64 - mean) / sd;
                        if z.abs() < 2.0 {
                            return None;
                        }
                        Some(InsightItem {
                            product_id: 0,
                            product_name: day.clone(),
                            batch_no: None,
                            quantity: Some(*v),
                            value: None,
                            days: Some(util::days_since(day)),
                            suggestion: Some(format!(
                                "{} units on {} is {:.1}σ {} the 30-day average of {:.1}/day.",
                                v,
                                day,
                                z.abs(),
                                if z > 0.0 { "above" } else { "below" },
                                mean
                            )),
                        })
                    })
                    .collect()
            };

            insights.push(Insight {
                id: "volatility".into(),
                title: "Sales volatility".into(),
                rule: "Daily units sold z-score over a 30-day window; flag days beyond ±2σ.".into(),
                severity: "info".into(),
                detail: if sd < 0.001 {
                    "Not enough variance in the last 30 days to flag anything.".to_string()
                } else {
                    format!("{} day(s) fell outside ±2σ of the 30-day mean.", items.len())
                },
                metric: Some(mean),
                metric_label: Some("avg units/day".into()),
                item_count: items.len(),
                items,
            });
        }

        Ok(Insights {
            generated_at: util::now_ts(),
            total_flagged: insights.iter().map(|i| i.item_count).sum(),
            insights,
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo;
    use crate::domain;
    use crate::models::{AdjustRequest, NewBatch, NewProduct, RestockRequest, SaleRequest};

    fn db() -> Db {
        crate::test_db().expect("in-memory db")
    }

    fn restock(db: &Db, name: &str, cat: &str, batch_no: &str, qty: i64, price: f64, expiry: &str) -> i64 {
        domain::restock(
            db,
            &RestockRequest {
                product: Some(NewProduct {
                    name: name.into(),
                    category: Some(cat.into()),
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

    fn days_from_today(n: i64) -> String {
        util::add_days(util::today(), n)
    }

    fn find<'a>(all: &'a Insights, id: &str) -> &'a Insight {
        all.insights
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| {
                panic!(
                    "no insight '{id}'; have {:?}",
                    all.insights.iter().map(|i| &i.id).collect::<Vec<_>>()
                )
            })
    }

    /// AC-9: every card must state the rule that produced it. An insight with an empty rule is
    /// an assertion the reader cannot check.
    #[test]
    fn ac9_every_insight_states_its_rule() {
        let db = db();
        restock(&db, "Amox", "Pharma", "A1", 10, 10.0, &days_from_today(10));
        let all = all(&db).expect("insights");

        assert_eq!(all.insights.len(), 8, "all 8 heuristics are emitted");
        for i in &all.insights {
            assert!(!i.rule.trim().is_empty(), "{} has no rule", i.id);
            assert!(i.rule.len() > 20, "{} rule is too thin to audit: {:?}", i.id, i.rule);
            assert!(!i.title.trim().is_empty(), "{} has no title", i.id);
            assert!(!i.detail.trim().is_empty(), "{} has no detail", i.id);
        }
    }

    /// Reorder maths is `avg_daily_usage(30d) x lead_time x 1.5 + reorder_point` for the trigger
    /// and `reorder_point + avg_daily_usage x lead_time - on_hand` for the suggested quantity.
    /// Both are documented in the card, so both are asserted here against a known history.
    #[test]
    fn reorder_suggestion_matches_the_documented_formula() {
        let db = db();
        let pid = restock(&db, "Fast Mover", "Pharma", "F1", 5, 4.0, &days_from_today(400));
        db.write(|tx| {
            tx.execute(
                "UPDATE products SET reorder_point = 40, lead_time_days = 7 WHERE id = ?1",
                [pid],
            )?;
            Ok(())
        })
        .unwrap();

        // 6 units/day for the last 30 days = 180 units out. The reorder query reads demand from
        // `usage_daily`, which `sell()` maintains — writing ledger rows alone would leave it
        // looking like a zero-demand product and take the "no observed demand" branch instead.
        db.write(|tx| {
            for day in 0..30 {
                repo::append_ledger(
                    tx,
                    &repo::LedgerRow {
                        id: 0,
                        ts: format!("{}T10:00:00Z", days_from_today(-day)),
                        kind: "sale".into(),
                        product_id: pid,
                        product_name: "Fast Mover".into(),
                        batch_id: None,
                        batch_no: None,
                        expiry_date: None,
                        qty_delta: -6,
                        unit_price: 4.0,
                        total_value: 24.0,
                        buyer_name: Some("Buyer".into()),
                        note: None,
                        source: "ui".into(),
                        actor: None,
                    },
                )?;
                repo::bump_usage_day(tx, pid, &days_from_today(-day), 6, 0)?;
            }
            Ok(())
        })
        .unwrap();

        let all = all(&db).expect("insights");
        let card = find(&all, "reorder");
        assert!(card.item_count > 0, "a fast mover below its reorder point must be flagged");

        let row = card.items.iter().find(|i| i.product_id == pid).expect("our product");
        assert_eq!(row.quantity, Some(98), "order quantity must be target - on_hand");

        // avg_daily = 180/30 = 6, lead_time = 7, reorder_point = 40
        //   target = ceil(6 * 7 * 1.5) + 40 = 103
        //   order  = ceil(103 - 5)          = 98
        let suggestion = row.suggestion.clone().unwrap_or_default();
        assert!(
            suggestion.contains("6.0/day"),
            "suggestion must state the observed daily rate, got {suggestion:?}"
        );
        assert!(
            suggestion.contains("98"),
            "suggested quantity 98 should appear in the suggestion, got {suggestion:?}"
        );
        assert_eq!(row.days, Some(0), "5 units at 6/day is under a day of cover");

        // AC-9: the card's printed rule must describe the arithmetic actually performed.
        let rule = card.rule.to_lowercase();
        assert!(rule.contains("1.5"), "rule should state the safety factor: {}", card.rule);
        assert!(rule.contains("reorder_point"), "rule should name its inputs: {}", card.rule);
    }

    /// Buyer leaderboard must report value as a positive magnitude. It previously negated
    /// `total_value`, which is already unsigned, so every buyer read as negative revenue.
    #[test]
    fn buyer_leaderboard_reports_positive_values() {
        let db = db();
        let pid = restock(&db, "Bought Thing", "Pharma", "B1", 50, 20.0, &days_from_today(300));
        domain::sell(
            &db,
            &SaleRequest {
                product_id: pid,
                quantity: 10,
                buyer_name: "Sunrise Medical".into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .unwrap();

        let all = all(&db).expect("insights");
        let card = find(&all, "buyers");
        let row = card.items.iter().find(|i| i.product_name == "Sunrise Medical").expect("buyer row");
        let value = row.value.expect("buyer value");
        assert_eq!(value, 200.0, "10 x 20.00 must read as positive revenue");
        assert_eq!(row.quantity, Some(10));
    }

    /// A negative stock adjustment is a write-off and must surface as dead/negative, not as an
    /// inbound. Guards the sign convention end-to-end through the ledger.
    #[test]
    fn negative_adjustment_is_recorded_as_an_outflow() {
        let db = db();
        let pid = restock(&db, "Breakable", "Pharma", "K1", 20, 3.0, &days_from_today(200));
        domain::adjust(
            &db,
            &AdjustRequest {
                product_id: pid,
                quantity_delta: -5,
                batch_no: Some("K1".into()),
                reason: Some("damaged in transit".into()),
                source: Some("ui".into()),
                actor: None,
            },
        )
        .unwrap();

        db.read(|c| {
            let (delta, value): (i64, f64) = c.query_row(
                "SELECT qty_delta, total_value FROM ledger WHERE kind='adjustment'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(delta, -5);
            assert_eq!(value, 15.0, "5 x 3.00 stored as a positive magnitude");
            let qty: i64 = c.query_row(
                "SELECT SUM(quantity) FROM batches WHERE product_id = ?1",
                [pid],
                |r| r.get(0),
            )?;
            assert_eq!(qty, 15);
            Ok(())
        })
        .unwrap();
    }

    /// Dead stock needs 90+ days without movement. A product touched yesterday must not appear.
    #[test]
    fn recently_moved_stock_is_not_reported_as_dead() {
        let db = db();
        let pid = restock(&db, "Fresh Item", "Pharma", "N1", 40, 2.0, &days_from_today(500));
        domain::sell(
            &db,
            &SaleRequest {
                product_id: pid,
                quantity: 1,
                buyer_name: "Today Buyer".into(),
                unit_price_override: None,
                allow_expired: false,
                note: None,
                source: Some("ui".into()),
                actor: None,
            },
        )
        .unwrap();

        let all = all(&db).expect("insights");
        let card = find(&all, "dead_stock");
        assert!(
            !card.items.iter().any(|i| i.product_id == pid),
            "a product with a movement today must not be dead stock"
        );
    }
}
