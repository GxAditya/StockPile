//! Demo data. A realistic pharmacy/FMCG catalogue makes the agent, FEFO allocation, and the
//! insight heuristics observable instead of theoretical.

use crate::db::repo;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::NewProduct;
use crate::util;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

struct SeedProduct {
    name: &'static str,
    category: &'static str,
    unit: &'static str,
    reorder: f64,
    lead: i64,
    batches: &'static [(&'static str, f64, i64, i64)], // batch_no, price, qty, expiry_in_days
}

static CATALOGUE: &[SeedProduct] = &[
    SeedProduct {
        name: "Amoxicillin 500mg Capsules",
        category: "Pharma",
        unit: "box",
        reorder: 40.0,
        lead: 7,
        batches: &[
            ("B2401", 12.50, 180, 320),
            ("B2402", 12.90, 96, 58),
            ("B2403", 12.75, 240, 12),
        ],
    },
    SeedProduct {
        name: "Panadol 500mg Tablets",
        category: "Pharma",
        unit: "strip",
        reorder: 60.0,
        lead: 5,
        batches: &[
            ("P1101", 8.00, 320, 410),
            ("P1102", 8.40, 150, 21),
            ("P1103", 8.25, 60, -4),
        ],
    },
    SeedProduct {
        name: "Paracetamol 650mg Strip",
        category: "Pharma",
        unit: "strip",
        reorder: 50.0,
        lead: 5,
        batches: &[("P2201", 7.80, 210, 265), ("P2202", 8.10, 140, 88)],
    },
    SeedProduct {
        name: "Vitamin C 500mg Chewable",
        category: "Supplements",
        unit: "bottle",
        reorder: 25.0,
        lead: 14,
        batches: &[
            ("V3301", 145.00, 42, 500),
            ("V3302", 152.00, 18, 33),
        ],
    },
    SeedProduct {
        name: "Calcium + Vitamin D3",
        category: "Supplements",
        unit: "bottle",
        reorder: 20.0,
        lead: 14,
        batches: &[("V3401", 210.00, 28, 380), ("V3402", 218.00, 9, 16)],
    },
    SeedProduct {
        name: "Hand Sanitizer 500ml",
        category: "Personal Care",
        unit: "pcs",
        reorder: 35.0,
        lead: 10,
        batches: &[("H5501", 95.00, 64, -12), ("H5502", 89.00, 120, 150)],
    },
    SeedProduct {
        name: "Face Wash 100ml",
        category: "Personal Care",
        unit: "pcs",
        reorder: 30.0,
        lead: 12,
        batches: &[("H5601", 175.00, 38, 340), ("H5602", 169.00, 22, 74)],
    },
    SeedProduct {
        name: "Surgical Face Mask (50pc)",
        category: "Medical Supplies",
        unit: "box",
        reorder: 45.0,
        lead: 6,
        batches: &[("M7701", 130.00, 90, 300), ("M7702", 135.00, 55, 45)],
    },
    SeedProduct {
        name: "N95 Respirator Mask",
        category: "Medical Supplies",
        unit: "box",
        reorder: 30.0,
        lead: 15,
        batches: &[("M7801", 480.00, 24, 420), ("M7802", 495.00, 11, 25)],
    },
    SeedProduct {
        name: "Digital Thermometer",
        category: "Medical Supplies",
        unit: "pcs",
        reorder: 15.0,
        lead: 21,
        batches: &[("M7901", 320.00, 26, 700), ("M7902", 305.00, 4, 190)],
    },
    SeedProduct {
        name: "Electrolyte Powder 400g",
        category: "Nutrition",
        unit: "pcs",
        reorder: 20.0,
        lead: 18,
        batches: &[("N8801", 265.00, 33, 290), ("N8802", 272.00, 12, 62)],
    },
    SeedProduct {
        name: "Protein Powder 1kg",
        category: "Nutrition",
        unit: "pcs",
        reorder: 12.0,
        lead: 24,
        batches: &[("N8901", 1450.00, 9, 240), ("N8902", 1495.00, 3, 40)],
    },
    SeedProduct {
        name: "ORS Sachet (10s)",
        category: "Pharma",
        unit: "box",
        reorder: 40.0,
        lead: 6,
        batches: &[("O9901", 42.00, 140, 200), ("O9902", 44.00, 60, 27)],
    },
    SeedProduct {
        name: "Betamethasone Cream",
        category: "Pharma",
        unit: "pcs",
        reorder: 10.0,
        lead: 9,
        batches: &[("B9901", 78.00, 17, 155)],
    },
    SeedProduct {
        name: "Loratadine 10mg",
        category: "Pharma",
        unit: "strip",
        reorder: 35.0,
        lead: 7,
        batches: &[("L4501", 11.20, 200, 330), ("L4502", 11.80, 75, 19)],
    },
];

static BUYERS: &[&str] = &[
    "Mr. Rajesh Sharma",
    "Priya Nair",
    "Sunrise Medical Store",
    "Dr. Anand Kumar",
    "Carewell Pharmacy",
    "Fatima Sheikh",
    "Green Cross Clinic",
    "Vikram Hospital Store",
    "Neha Gupta",
    "Apollo Franchise Outlet",
];

static SUPPLIERS: &[&str] = &[
    "MedPlus Distribution",
    "Healthline Pharma",
    "Sunrise Suppliers",
    "Apex Medical Corp",
];

pub fn is_seeded(db: &Db) -> AppResult<bool> {
    db.read(|c| {
        let n: i64 = c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        Ok(n > 0)
    })
}

/// Build a coherent 120-day history: daily sales weighted by a per-product demand factor, plus
/// receipts landing before the sales they enable.
pub fn seed(db: &Db) -> AppResult<SeedReport> {
    let mut rng = StdRng::seed_from_u64(0x570C_11E5);

    let today = util::today();
    let mut products_created = 0usize;
    let mut batches_created = 0usize;

    // Suppliers/buyers are written with the `source = seed` marker so history is attributable.
    for p in CATALOGUE {
        let exists = db.read(|c| repo::find_product_by_name(c, p.name))?;
        let product = match exists {
            Some(existing) => existing,
            None => {
                let created = db.write(|c| {
                    repo::create_product(
                        c,
                        &NewProduct {
                            name: p.name.to_string(),
                            sku: Some(
                                p.name
                                    .chars()
                                    .filter(|ch| ch.is_ascii_alphanumeric())
                                    .take(6)
                                    .collect::<String>()
                                    .to_uppercase(),
                            ),
                            barcode: None,
                            category: Some(p.category.to_string()),
                            unit: Some(p.unit.to_string()),
                            description: None,
                            reorder_point: Some(p.reorder),
                            lead_time_days: Some(p.lead),
                        },
                    )
                })?;
                products_created += 1;
                created
            }
        };

        // Batches arrive progressively, oldest first, so FEFO ordering is genuinely exercised.
        let mut ordered: Vec<&(&str, f64, i64, i64)> = p.batches.iter().collect();
        ordered.sort_by_key(|(_, _, _, days)| -*days);

        for (batch_no, price, qty, expiry_in) in ordered {
            let expiry = util::add_days(today, *expiry_in);
            // Received 60-95 days before expiry so `received_at` precedes `expiry_date`.
            let received = util::add_days(util::parse_date(&expiry).unwrap_or(today), -(60 + rng.gen_range(0..35)));

            db.write(|c| {
                let conn = c;
                conn.execute(
                    "INSERT INTO batches (product_id, batch_no, expiry_date, unit_price, quantity,
                                          received_at, supplier, notes)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,NULL)",
                    rusqlite::params![
                        product.id,
                        batch_no,
                        expiry,
                        price,
                        qty,
                        received,
                        SUPPLIERS[rng.gen_range(0..SUPPLIERS.len())]
                    ],
                )?;
                let batch_id = conn.last_insert_rowid();
                repo::append_ledger(
                    conn,
                    &repo::LedgerRow {
                        id: 0,
                        ts: format!("{received}T09:15:00Z"),
                        kind: "restock".into(),
                        product_id: product.id,
                        product_name: product.name.clone(),
                        batch_id: Some(batch_id),
                        batch_no: Some(batch_no.to_string()),
                        expiry_date: Some(expiry.clone()),
                        qty_delta: *qty,
                        unit_price: *price,
                        total_value: (*qty as f64) * price,
                        buyer_name: None,
                        note: Some("Opening stock".into()),
                        source: "seed".into(),
                        actor: None,
                    },
                )?;
                repo::bump_usage_day(conn, product.id, &received, 0, *qty)?;
                Ok(())
            })?;
            batches_created += 1;
        }
    }

    // ── Sales history ─────────────────────────────────────────────────────────
    // Walk the last 120 days. Each day, for each product with stock, sell a small number of
    // units proportional to that product's popularity, skipping some days to create variance
    // the volatility heuristic can actually flag.
    let ids: Vec<(i64, String, String, String, i64, f64)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT p.id, p.name, p.unit, p.category,
                    COALESCE(SUM(b.quantity),0), COALESCE(SUM(b.quantity*b.unit_price),0)
             FROM products p LEFT JOIN batches b ON b.product_id = p.id
             GROUP BY p.id",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok::<_, AppError>(rows)
    })?;

    for day_offset in (0..120).rev() {
        let day = util::add_days(today, -day_offset);
        let dow = util::parse_date(&day).map(|d| chrono::Datelike::weekday(&d).num_days_from_monday());

        for (idx, (pid, pname, _punit, _cat, _onhand, _value)) in ids.iter().enumerate() {
            // Weekends sell less. Popularity varies by product so the charts are not flat.
            let weekend_factor = match dow {
                Some(5) | Some(6) => 0.45,
                _ => 1.0,
            };
            let popularity = 1.0 + ((idx * 37) % 11) as f64 * 0.35;
            let expected = rng.gen_range(0.0..3.4) * weekend_factor * popularity;
            if expected < 0.35 {
                continue;
            }
            let qty = expected.round() as i64;
            if qty < 1 {
                continue;
            }
            let buyer = BUYERS[rng.gen_range(0..BUYERS.len())];
            let hour = 9 + rng.gen_range(0..10);
            let minute = rng.gen_range(0..60);
            let ts = format!("{day}T{hour:02}:{minute:02}:00Z");

            // Backdated FEFO: only batches that had actually arrived by `day` and were not yet
            // expired on `day`. Everything happens in one transaction so seed data satisfies the
            // same ledger invariant as live sales.
            let applied = db.write(|tx| {
                let conn = tx;
                let batches = sellable_batches(conn, *pid, &day)?;

                if batches.iter().map(|b| b.4).sum::<i64>() < qty {
                    return Ok::<bool, AppError>(false);
                }

                let mut remaining = qty;
                for (bid, batch_no, expiry, price, available) in batches {
                    if remaining <= 0 {
                        break;
                    }
                    let take = remaining.min(available);
                    conn.execute(
                        "UPDATE batches SET quantity = quantity - ?1 WHERE id = ?2",
                        rusqlite::params![take, bid],
                    )?;
                    let line = (take as f64 * price * 100.0).round() / 100.0;
                    repo::append_ledger(
                        conn,
                        &repo::LedgerRow {
                            id: 0,
                            ts: ts.clone(),
                            kind: "sale".into(),
                            product_id: *pid,
                            product_name: pname.clone(),
                            batch_id: Some(bid),
                            batch_no: Some(batch_no.clone()),
                            expiry_date: expiry.clone(),
                            qty_delta: -take,
                            unit_price: price,
                            total_value: line,
                            buyer_name: Some(buyer.to_string()),
                            note: None,
                            source: "seed".into(),
                            actor: None,
                        },
                    )?;
                    remaining -= take;
                }
                repo::bump_usage_day(conn, *pid, &day, qty, 0)?;
                Ok(true)
            })?;

            let _ = applied;
        }
    }

    let totals = db.read(|c| {
        let products: i64 = c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        let batches: i64 = c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))?;
        let sale_rows: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE kind='sale'", [], |r| r.get(0))?;
        Ok::<_, AppError>((products, batches, sale_rows))
    })?;

    let _ = products_created;
    let _ = batches_created;

    Ok(SeedReport {
        products: totals.0,
        batches: totals.1,
        sales: totals.2,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedReport {
    pub products: i64,
    pub batches: i64,
    pub sales: i64,
}
/// Batches that had arrived, and had not expired, as of `day` — the backdated FEFO candidate
/// set for seeding.
fn sellable_batches(
    conn: &rusqlite::Connection,
    product_id: i64,
    day: &str,
) -> AppResult<Vec<(i64, String, Option<String>, f64, i64)>> {
    let mut st = conn.prepare(
        "SELECT id, batch_no, expiry_date, unit_price, quantity FROM batches
         WHERE product_id = ?1 AND quantity > 0 AND received_at <= ?2
           AND (expiry_date IS NULL OR expiry_date >= ?2)
         ORDER BY CASE WHEN expiry_date IS NULL THEN 1 ELSE 0 END,
                  expiry_date ASC, id ASC",
    )?;
    let rows = st
        .query_map(rusqlite::params![product_id, day], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
