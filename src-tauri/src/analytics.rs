//! Dashboard aggregates. All computed by SQL at read time — no denormalised counters that
//! can drift away from the ledger.

use crate::db::Db;
use crate::error::AppResult;
use crate::util;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kpis {
    pub total_units: i64,
    pub total_value: f64,
    pub product_count: i64,
    pub live_batch_count: i64,
    pub low_stock_count: i64,
    pub out_of_stock_count: i64,
    pub expiring_7d_batches: i64,
    pub expiring_30d_batches: i64,
    pub expired_batches: i64,
    pub revenue_30d: f64,
    pub units_sold_30d: i64,
    pub units_added_30d: i64,
    pub distinct_buyers_30d: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryValue {
    pub category: String,
    pub value: f64,
    pub units: i64,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopProduct {
    pub product_id: i64,
    pub name: String,
    pub units: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizonBucket {
    pub bucket: String,
    pub batches: i64,
    pub units: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionItem {
    pub batch_id: i64,
    pub product_id: i64,
    pub product_name: String,
    pub batch_no: String,
    pub expiry_date: String,
    pub days_remaining: i64,
    pub quantity: i64,
    pub unit_price: f64,
    pub value_at_risk: f64,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub kpis: Kpis,
    pub top_sold_30d: Vec<TopProduct>,
    pub expiry_horizon: Vec<HorizonBucket>,
    pub needs_attention: Vec<AttentionItem>,
    pub generated_at: String,
}

const HEALTH_CTE: &str = r#"
WITH stock AS (
    SELECT p.id,
           COALESCE(SUM(b.quantity), 0) AS total_quantity,
           COALESCE(SUM(b.quantity * b.unit_price), 0) AS total_value,
           COALESCE(SUM(CASE WHEN b.quantity > 0 AND b.expiry_date IS NOT NULL
                             AND b.expiry_date < :today THEN b.quantity ELSE 0 END), 0) AS expired_units,
           COALESCE(SUM(CASE WHEN b.quantity > 0 AND b.expiry_date IS NOT NULL
                             AND b.expiry_date >= :today AND b.expiry_date <= :soon
                             THEN b.quantity ELSE 0 END), 0) AS expiring_units
    FROM products p
    LEFT JOIN batches b ON b.product_id = p.id
    GROUP BY p.id
)
"#;

pub fn dashboard(db: &Db) -> AppResult<Dashboard> {
    let today = util::today();
    let today_s = util::today_str();
    // Forward-looking horizons (expiry).
    let d7 = util::add_days(today, 7);
    let d30 = util::add_days(today, 30);
    // Backward-looking windows (sales). These are *past* dates and must never be confused with
    // the expiry horizons above — mixing them up silently returned an empty "top sellers" chart.
    let d30_back = util::add_days(today, -29);

    db.read(|conn| {
        // ── KPIs ──────────────────────────────────────────────────────────────────
        let (total_units, total_value): (i64, f64) = conn.query_row(
            "SELECT COALESCE(SUM(quantity),0), COALESCE(SUM(quantity*unit_price),0)
             FROM batches WHERE quantity > 0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let product_count: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        let live_batch_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM batches WHERE quantity > 0", [], |r| r.get(0))?;

        let (low_stock_count, out_of_stock_count): (i64, i64) = conn.query_row(
            &format!(
                "{HEALTH_CTE}
                 SELECT
                   COALESCE(SUM(CASE WHEN total_quantity > 0 AND total_quantity <= p.reorder_point
                                     THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN total_quantity = 0 THEN 1 ELSE 0 END), 0)
                 FROM stock s JOIN products p ON p.id = s.id"
            ),
            rusqlite::params![today_s.clone(), d30.clone()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let (expired_batches, expiring_7d_batches, expiring_30d_batches) = conn.query_row(
            "SELECT
               COALESCE(SUM(CASE WHEN expiry_date < ?1 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN expiry_date >= ?1 AND expiry_date <= ?2 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN expiry_date >= ?1 AND expiry_date <= ?3 THEN 1 ELSE 0 END), 0)
             FROM batches WHERE quantity > 0 AND expiry_date IS NOT NULL",
            rusqlite::params![today_s.clone(), d7.clone(), d30.clone()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        let (revenue_30d, units_sold_30d, units_added_30d, distinct_buyers_30d): (f64, i64, i64, i64) =
            conn.query_row(
            "SELECT
               -- `total_value` is a magnitude; direction comes from `qty_delta`.
               COALESCE(SUM(CASE WHEN kind='sale'    THEN total_value ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN kind='sale'    THEN -qty_delta ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN kind='restock' THEN  qty_delta ELSE 0 END), 0),
               COUNT(DISTINCT CASE WHEN kind='sale' THEN buyer_name END)
             FROM ledger WHERE substr(ts,1,10) >= ?1",
            [&d30_back],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;

        let kpis = Kpis {
            total_units,
            total_value,
            product_count,
            live_batch_count,
            low_stock_count,
            out_of_stock_count,
            expiring_7d_batches,
            expiring_30d_batches,
            expired_batches,
            revenue_30d: (revenue_30d * 100.0).round() / 100.0,
            units_sold_30d,
            units_added_30d,
            distinct_buyers_30d,
        };

        // ── Top sold, 30 days ──────────────────────────────────────────────────────
        let mut stmt = conn.prepare(
            "SELECT product_id, product_name, SUM(-qty_delta) AS units, SUM(total_value) AS value
             FROM ledger
             WHERE kind='sale' AND substr(ts,1,10) >= ?1
             GROUP BY product_id, product_name
             ORDER BY units DESC LIMIT 5",
        )?;
        let top_sold_30d = stmt
            .query_map([&d30_back], |r| {
                Ok(TopProduct {
                    product_id: r.get(0)?,
                    name: r.get(1)?,
                    units: r.get(2)?,
                    value: (r.get::<_, f64>(3)? * 100.0).round() / 100.0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // ── Expiry horizon ─────────────────────────────────────────────────────────
        let horizon_specs: [(&str, String, String); 5] = [
            ("Expired", "-9999".to_string(), today_s.clone()),
            ("Within 7 days", today_s.clone(), d7.clone()),
            ("8-30 days", util::add_days(today, 7), d30.clone()),
            ("31-90 days", util::add_days(today, 30), util::add_days(today, 90)),
            ("Beyond 90 days", util::add_days(today, 90), "9999-12-31".to_string()),
        ];
        let mut expiry_horizon = Vec::new();
        for (label, lo, hi) in horizon_specs {
            let (batches, units, value): (i64, i64, f64) = conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(quantity),0), COALESCE(SUM(quantity*unit_price),0)
                 FROM batches
                 WHERE quantity > 0 AND expiry_date IS NOT NULL AND expiry_date >= ?1 AND expiry_date <= ?2",
                rusqlite::params![lo, hi],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            expiry_horizon.push(HorizonBucket {
                bucket: label.to_string(),
                batches,
                units,
                value: (value * 100.0).round() / 100.0,
            });
        }

        // ── Needs attention ────────────────────────────────────────────────────────
        let mut stmt = conn.prepare(
            "SELECT b.id, b.product_id, p.name, b.batch_no, b.expiry_date,
                    julianday(b.expiry_date) - julianday(?1),
                    b.quantity, b.unit_price, b.quantity * b.unit_price
             FROM batches b JOIN products p ON p.id = b.product_id
             WHERE b.quantity > 0 AND b.expiry_date IS NOT NULL
               AND b.expiry_date <= ?2
             ORDER BY b.expiry_date ASC LIMIT 12",
        )?;
        let needs_attention = stmt
            .query_map(rusqlite::params![today_s.clone(), d30.clone()], |r| {
                let days: f64 = r.get(5)?;
                let days_remaining = days.floor() as i64;
                Ok(AttentionItem {
                    batch_id: r.get(0)?,
                    product_id: r.get(1)?,
                    product_name: r.get(2)?,
                    batch_no: r.get(3)?,
                    expiry_date: r.get(4)?,
                    days_remaining,
                    quantity: r.get(6)?,
                    unit_price: r.get(7)?,
                    value_at_risk: (r.get::<_, f64>(8)? * 100.0).round() / 100.0,
                    severity: if days_remaining < 0 {
                        "expired".into()
                    } else if days_remaining <= 7 {
                        "critical".into()
                    } else {
                        "warning".into()
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Dashboard {
            kpis,
            top_sold_30d,
            expiry_horizon,
            needs_attention,
            generated_at: util::now_ts(),
        })
    })
}