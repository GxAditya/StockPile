//! Tool registry. The agent has no SQL or filesystem access — it may only call tools from this
//! closed enum, and every argument is validated against a typed schema before it can reach the
//! domain layer. Read tools execute immediately; write tools are always held behind a
//! confirmation gate (PRD §8.4).

use super::ollama::ToolCall;
use crate::db::repo;
use crate::db::Db;
use crate::domain;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::util;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    SearchProducts,
    GetStock,
    ListExpiring,
    RecentActivity,
    StockValueByCategory,
    LowStock,
    TopSellers,
    RunReadonlyQuery,
    CreateProduct,
    AddBatch,
    SellStock,
    AdjustStock,
}

impl Tool {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "search_products" => Tool::SearchProducts,
            "get_stock" => Tool::GetStock,
            "list_expiring" => Tool::ListExpiring,
            "recent_activity" => Tool::RecentActivity,
            "stock_value_by_category" => Tool::StockValueByCategory,
            "low_stock" => Tool::LowStock,
            "top_sellers" => Tool::TopSellers,
            "run_readonly_query" | "run_query" | "query_sql" => Tool::RunReadonlyQuery,
            "create_product" => Tool::CreateProduct,
            "add_batch" => Tool::AddBatch,
            "sell_stock" => Tool::SellStock,
            "adjust_stock" => Tool::AdjustStock,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::SearchProducts => "search_products",
            Tool::GetStock => "get_stock",
            Tool::ListExpiring => "list_expiring",
            Tool::RecentActivity => "recent_activity",
            Tool::StockValueByCategory => "stock_value_by_category",
            Tool::LowStock => "low_stock",
            Tool::TopSellers => "top_sellers",
            Tool::RunReadonlyQuery => "run_readonly_query",
            Tool::CreateProduct => "create_product",
            Tool::AddBatch => "add_batch",
            Tool::SellStock => "sell_stock",
            Tool::AdjustStock => "adjust_stock",
        }
    }

    pub fn is_write(self) -> bool {
        matches!(
            self,
            Tool::CreateProduct | Tool::AddBatch | Tool::SellStock | Tool::AdjustStock
        )
    }

    pub fn description(self) -> &'static str {
        match self {
            Tool::SearchProducts => "Find products by name, SKU, or category. Use this before any write tool if you are not certain of the product.",
            Tool::GetStock => "Current on-hand quantity, batch count, average cost, and earliest expiry for one product.",
            Tool::ListExpiring => "Live batches expiring within a number of days, with quantity and value at risk.",
            Tool::RecentActivity => "Recent ledger entries: restocks and sales, optionally filtered by product or buyer.",
            Tool::StockValueByCategory => "Total inventory value grouped by category.",
            Tool::LowStock => "Products at or below their reorder point, or with zero stock.",
            Tool::TopSellers => "Best selling products over a number of days, by units and value.",
            Tool::RunReadonlyQuery => "Full read access to ALL inventory data. Run any single SELECT/WITH query over products, batches, ledger, usage_daily (plus settings, agent_messages). Use this when no narrow tool answers the question — joins, aggregates, trends, comparisons, full-table scans. Read-only: writes are rejected.",
            Tool::CreateProduct => "Create a new product. Only use this when the product genuinely does not exist yet.",
            Tool::AddBatch => "Receive stock: add a batch with batch number, expiry, unit price and quantity to an existing product.",
            Tool::SellStock => "Sell stock from a product to a named buyer. Allocates earliest-expiry-first.",
            Tool::AdjustStock => "Correct stock that drifted: damage, miscount, or write-off. Requires a reason.",
        }
    }

    pub fn schema(self) -> Value {
        let obj = |props: Value, required: Value| {
            json!({ "type": "object", "properties": props, "required": required })
        };
        match self {
            Tool::SearchProducts => obj(
                json!({ "query": {"type": "string", "description": "Product name, SKU, or category fragment"} }),
                json!(["query"]),
            ),
            Tool::GetStock => obj(
                json!({ "name": {"type": "string", "description": "Product name or SKU"} }),
                json!(["name"]),
            ),
            Tool::ListExpiring => obj(
                json!({ "within_days": {"type": "integer", "description": "Days from today. 0 means already expired. Default 30."} }),
                json!([]),
            ),
            Tool::RecentActivity => obj(
                json!({
                    "limit": {"type": "integer"},
                    "product_id": {"type": "integer"},
                    "buyer_name": {"type": "string"},
                    "days": {"type": "integer", "description": "How far back to look. Default 30."}
                }),
                json!([]),
            ),
            Tool::StockValueByCategory => obj(json!({}), json!([])),
            Tool::LowStock => obj(json!({}), json!([])),
            Tool::TopSellers => obj(
                json!({ "days": {"type": "integer", "description": "Default 30."}, "limit": {"type": "integer", "description": "Max rows. Default 50."} }),
                json!([]),
            ),
            Tool::RunReadonlyQuery => obj(
                json!({ "sql": {"type": "string", "description": "A single SELECT or WITH query. May read products, batches, ledger, usage_daily, settings, agent_messages. Must include a LIMIT or one is added (cap 500 rows). No INSERT/UPDATE/DELETE/DDL."} }),
                json!(["sql"]),
            ),
            Tool::CreateProduct => obj(
                json!({
                    "name": {"type": "string"},
                    "category": {"type": "string"},
                    "unit": {"type": "string", "description": "pcs, box, kg, bottle..."},
                    "sku": {"type": "string"},
                    "reorder_point": {"type": "number"},
                    "lead_time_days": {"type": "integer"}
                }),
                json!(["name"]),
            ),
            Tool::AddBatch => obj(
                json!({
                    "name": {"type": "string", "description": "Product name or SKU. Must already exist."},
                    "batch_no": {"type": "string"},
                    "quantity": {"type": "integer"},
                    "unit_price": {"type": "number"},
                    "expiry_date": {"type": "string", "description": "YYYY-MM-DD"},
                    "supplier": {"type": "string"}
                }),
                json!(["name", "batch_no", "quantity", "unit_price"]),
            ),
            Tool::SellStock => obj(
                json!({
                    "name": {"type": "string"},
                    "quantity": {"type": "integer"},
                    "buyer_name": {"type": "string"},
                    "unit_price_override": {"type": "number"}
                }),
                json!(["name", "quantity", "buyer_name"]),
            ),
            Tool::AdjustStock => obj(
                json!({
                    "name": {"type": "string"},
                    "quantity_delta": {"type": "integer", "description": "Negative to remove, positive to add."},
                    "batch_no": {"type": "string"},
                    "reason": {"type": "string"}
                }),
                json!(["name", "quantity_delta", "reason"]),
            ),
        }
    }
}

pub const ALL_TOOLS: &[Tool] = &[
    Tool::SearchProducts,
    Tool::GetStock,
    Tool::ListExpiring,
    Tool::RecentActivity,
    Tool::StockValueByCategory,
    Tool::LowStock,
    Tool::TopSellers,
    Tool::RunReadonlyQuery,
    Tool::CreateProduct,
    Tool::AddBatch,
    Tool::SellStock,
    Tool::AdjustStock,
];

/// Read-only tools offered to the model. Write tools are added only once the model has shown it
/// understands a mutation was asked for — offering them always invites a small model to invent a
/// write it was never asked to make.
pub fn read_tool_defs() -> Vec<Value> {
    ALL_TOOLS
        .iter()
        .filter(|t| !t.is_write())
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.schema(),
                }
            })
        })
        .collect()
}

pub fn all_tool_defs() -> Vec<Value> {
    ALL_TOOLS
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.schema(),
                }
            })
        })
        .collect()
}

// ── Argument validation ────────────────────────────────────────────────────────────────
// Small models send numbers as strings ("40"), booleans as "yes", and misspell field names.
// Coerce rather than reject, but reject anything that cannot be made sense of.

fn arg_str(args: &Value, key: &str) -> Option<String> {
    for k in [key, key.replace('_', " ").to_lowercase().as_str()] {
        match args.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() => return Some(s.trim().to_string()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    for k in [key, key.replace('_', " ").to_lowercase().as_str()] {
        match args.get(k) {
            Some(Value::Number(n)) => return n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
            Some(Value::String(s)) => {
                if let Ok(v) = s.trim().parse::<i64>() {
                    return Some(v);
                }
                // Pull the first run of digits: "40 units" -> 40
                let digits: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
                if let Ok(v) = digits.parse::<i64>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn arg_f64(args: &Value, key: &str) -> Option<f64> {
    for k in [key, key.replace('_', " ").to_lowercase().as_str()] {
        match args.get(k) {
            Some(Value::Number(n)) => return n.as_f64(),
            Some(Value::String(s)) => {
                let cleaned: String = s
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                    .collect();
                if let Ok(v) = cleaned.parse::<f64>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

/// A validated, ready-to-execute tool invocation.
#[derive(Debug, Clone)]
pub struct PendingCall {
    pub tool: Tool,
    pub args: Value,
    /// Human-readable one-line summary shown on the confirmation card.
    pub summary: String,
    /// Compact projection of the result handed to the model for narration.
    pub projection: String,
}

#[derive(Debug, Clone)]
pub struct Execution {
    pub summary: String,
    pub projection: String,
    pub changed: bool,
}

/// Result of a read tool: rows for display plus a compact string for the model.
pub struct ReadResult {
    pub summary: String,
    pub rows: Vec<Value>,
}

fn resolve_product(db: &Db, args: &Value) -> AppResult<(i64, String)> {
    let name = arg_str(args, "name")
        .or_else(|| arg_str(args, "product"))
        .or_else(|| arg_str(args, "product_name"))
        .ok_or_else(|| AppError::validation("a product name is required"))?;
    let matches = db.read(|c| repo::search_products(c, &name, 5))?;
    match matches.len() {
        0 => Err(AppError::NotFound(format!("no product matches '{name}'"))),
        1 => Ok((matches[0].id, matches[0].name.clone())),
        _ => {
            let exact = matches.iter().position(|m| {
                m.name.eq_ignore_ascii_case(&name) || m.sku.as_deref() == Some(name.as_str())
            });
            match exact {
                Some(i) => Ok((matches[i].id, matches[i].name.clone())),
                None => Err(AppError::validation(format!(
                    "'{name}' is ambiguous — candidates: {}",
                    matches
                        .iter()
                        .map(|m| m.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
            }
        }
    }
}

/// Execute a **read** tool. Never mutates state.
pub fn run_read(db: &Db, tool: Tool, args: &Value) -> AppResult<ReadResult> {
    match tool {
        Tool::SearchProducts => {
            let q = arg_str(args, "query")
                .or_else(|| arg_str(args, "q"))
                .or_else(|| arg_str(args, "name"))
                .unwrap_or_default();
            // Local model: give it room to explore the catalogue. Default 20, up to 100.
            let limit = arg_i64(args, "limit").unwrap_or(20).clamp(1, 100);
            let rows = db.read(|c| repo::search_products(c, &q, limit))?;
            let out: Vec<Value> = db.read(|c| {
                let mut st = c.prepare("SELECT id, name, COALESCE(SUM(quantity),0) q FROM batches WHERE product_id = ?1 GROUP BY product_id")?;
                let mut acc = Vec::new();
                for p in &rows {
                    let qty: i64 = st.query_row([p.id], |r| r.get(2)).unwrap_or(0);
                    acc.push(json!({"id": p.id, "name": p.name, "category": p.category, "unit": p.unit, "onHand": qty}));
                }
                Ok::<_, AppError>(acc)
            })?;
            Ok(ReadResult {
                summary: format!("{} product(s) matched '{q}'", out.len()),
                rows: out,
            })
        }

        Tool::GetStock => {
            let (id, name) = resolve_product(db, args)?;
            let list = db.read(|c| {
                repo::stock_list(
                    c,
                    &repo::StockQuery {
                        search: None,
                        category: None,
                        health: None,
                        sort: None,
                        limit: Some(5000),
                        offset: None,
                    },
                )
            })?;
            let row = list
                .rows
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| AppError::NotFound(format!("no stock row for '{name}'")))?;
            let batches = db.read(|c| repo::list_batches(c, id))?;
            // One homogeneous shape for every row. Prepending a product-level summary row with
            // different keys made the renderer show a wall of em-dashes under columns that do
            // not apply to that row, which reads like missing data rather than a mixed payload.
            // Row 0 is the product total; `unitPrice` there is the weighted-average cost, which
            // the column header makes ambiguous on its own, hence the explicit marker.
            let head = json!({
                "batchNo": "TOTAL (avg cost)",
                "expiry": row.earliest_expiry,
                "unitPrice": row.avg_cost,
                "quantity": row.total_quantity,
                "liveBatches": row.batch_count,
                "totalValue": row.total_value,
                "health": row.health.as_str(),
            });
            let mut all = vec![head];
            // Full read access: show every batch (including depleted ones) so the model can
            // see batch history, not just live stock. Capped at 200 to bound context.
            all.extend(batches.iter().take(200).map(|b| {
                json!({
                    "batchNo": b.batch_no,
                    "expiry": b.expiry_date,
                    "unitPrice": b.unit_price,
                    "quantity": b.quantity,
                    "liveBatches": Value::Null,
                    "totalValue": Value::Null,
                    "health": Value::Null,
                })
            }));
            Ok(ReadResult {
                summary: format!(
                    "{}: {} {} on hand across {} live batch(es)",
                    row.name, row.total_quantity, row.unit, row.batch_count
                ),
                rows: all,
            })
        }

        Tool::ListExpiring => {
            let within = arg_i64(args, "within_days").unwrap_or(30).clamp(0, 3650);
            let limit = arg_i64(args, "limit").unwrap_or(200).clamp(1, 500);
            let horizon = util::add_days(util::today(), within);
            let today = util::today_str();
            let rows: Vec<Value> = db.read(|c| {
                let mut st = c.prepare(
                    "SELECT p.id, p.name, b.batch_no, b.expiry_date, b.quantity, b.unit_price,
                            CAST(julianday(b.expiry_date) - julianday(?1) AS INTEGER) AS d
                     FROM batches b JOIN products p ON p.id = b.product_id
                     WHERE b.quantity > 0 AND b.expiry_date IS NOT NULL AND b.expiry_date <= ?2
                     ORDER BY b.expiry_date ASC LIMIT ?3",
                )?;
                let acc = st
                    .query_map(rusqlite::params![today.clone(), horizon.clone(), limit], |r| {
                        let qty: i64 = r.get(4)?;
                        let price: f64 = r.get(5)?;
                        Ok(json!({
                            "productId": r.get::<_, i64>(0)?,
                            "name": r.get::<_, String>(1)?,
                            "batchNo": r.get::<_, String>(2)?,
                            "expiry": r.get::<_, String>(3)?,
                            "quantity": qty,
                            "daysRemaining": r.get::<_, i64>(6)?,
                            "valueAtRisk": (qty as f64 * price * 100.0).round() / 100.0
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, AppError>(acc)
            })?;
            let units: i64 = rows
                .iter()
                .map(|r| r["quantity"].as_i64().unwrap_or(0))
                .sum();
            Ok(ReadResult {
                summary: format!(
                    "{} batch(es) within {within} day(s) of expiry, {units} unit(s) total",
                    rows.len()
                ),
                rows,
            })
        }

        Tool::RecentActivity => {
            let days = arg_i64(args, "days").unwrap_or(30).clamp(1, 3650);
            let from = util::add_days(util::today(), -days);
            let mut filter = HistoryFilter {
                from: Some(from),
                limit: Some(arg_i64(args, "limit").unwrap_or(50).clamp(1, 500)),
                ..Default::default()
            };
            filter.product_id = arg_i64(args, "product_id");
            filter.buyer_name = arg_str(args, "buyer_name")
                .or_else(|| arg_str(args, "buyer"));
            let page = db.read(|c| repo::history_flat(c, &filter))?;
            let rows: Vec<Value> = page
                .iter()
                .map(|e| {
                    json!({
                        "date": e.ts.chars().take(10).collect::<String>(),
                        "kind": e.kind,
                        "product": e.product_name,
                        "batchNo": e.batch_no,
                        "qtyDelta": e.qty_delta,
                        "unitPrice": e.unit_price,
                        "totalValue": e.total_value,
                        "buyer": e.buyer_name,
                        "source": e.source
                    })
                })
                .collect();
            Ok(ReadResult {
                summary: format!("{} ledger entr(ies) in the last {days} day(s)", rows.len()),
                rows,
            })
        }

        Tool::StockValueByCategory => {
            // Self-contained query rather than borrowing a Dashboard field: the dashboard no
            // longer renders a category breakdown, but the agent still answers "value by
            // category", so the data has to live wherever the agent can reach it.
            let limit = arg_i64(args, "limit").unwrap_or(50).clamp(1, 100);
            let rows: Vec<Value> = db.read(|c| {
                let grand: f64 = c.query_row(
                    "SELECT COALESCE(SUM(quantity * unit_price), 0) FROM batches WHERE quantity > 0",
                    [],
                    |r| r.get(0),
                )?;
                let mut stmt = c.prepare(
                    "SELECT COALESCE(NULLIF(p.category, ''), 'Uncategorised'),
                            COALESCE(SUM(b.quantity * b.unit_price), 0),
                            COALESCE(SUM(b.quantity), 0)
                     FROM batches b JOIN products p ON p.id = b.product_id
                     WHERE b.quantity > 0
                     GROUP BY 1 ORDER BY 2 DESC LIMIT ?1",
                )?;
                let acc = stmt
                    .query_map([limit], |r| {
                        let v: f64 = r.get(1)?;
                        Ok(json!({
                            "category": r.get::<_, String>(0)?,
                            "value": (v * 100.0).round() / 100.0,
                            "units": r.get::<_, i64>(2)?,
                            "sharePct": if grand > 0.0 { (v / grand * 1000.0).round() / 10.0 } else { 0.0 }
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, AppError>(acc)
            })?;
            Ok(ReadResult {
                summary: format!("{} category/categories by stock value", rows.len()),
                rows,
            })
        }

        Tool::LowStock => {
            let limit = arg_i64(args, "limit").unwrap_or(200).clamp(1, 500);
            let list = db.read(|c| {
                repo::stock_list(
                    c,
                    &repo::StockQuery {
                        search: None,
                        category: None,
                        health: Some("low".into()),
                        sort: Some("totalQuantity".into()),
                        limit: Some(limit),
                        offset: None,
                    },
                )
            })?;
            let out = db.read(|c| {
                let out_of = repo::stock_list(
                    c,
                    &repo::StockQuery {
                        search: None,
                        category: None,
                        health: Some("out".into()),
                        sort: Some("name".into()),
                        limit: Some(limit),
                        offset: None,
                    },
                )?;
                Ok::<_, AppError>((out_of, ()))
            })?;
            let mut rows: Vec<Value> = list
                .rows
                .iter()
                .map(|r| {
                    json!({
                        "name": r.name, "onHand": r.total_quantity,
                        "reorderPoint": r.reorder_point, "unit": r.unit, "health": "low"
                    })
                })
                .collect();
            rows.extend(out.0.rows.iter().map(|r| {
                json!({
                    "name": r.name, "onHand": 0,
                    "reorderPoint": r.reorder_point, "unit": r.unit, "health": "out"
                })
            }));
            Ok(ReadResult {
                summary: format!("{} product(s) at or below reorder point", rows.len()),
                rows,
            })
        }

        Tool::TopSellers => {
            let days = arg_i64(args, "days").unwrap_or(30).clamp(1, 3650);
            let limit = arg_i64(args, "limit").unwrap_or(50).clamp(1, 100);
            let from = util::add_days(util::today(), -days);
            let rows: Vec<Value> = db.read(|c| {
                let mut st = c.prepare(
                    "SELECT product_name, SUM(-qty_delta) AS units, SUM(total_value) AS value
                     FROM ledger WHERE kind='sale' AND substr(ts,1,10) >= ?1
                     GROUP BY product_name ORDER BY units DESC LIMIT ?2",
                )?;
                let acc = st
                    .query_map(rusqlite::params![from.clone(), limit], |r| {
                        Ok(json!({
                            "name": r.get::<_, String>(0)?,
                            "units": r.get::<_, i64>(1)?,
                            "value": (r.get::<_, f64>(2)? * 100.0).round() / 100.0
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, AppError>(acc)
            })?;
            Ok(ReadResult {
                summary: format!("top sellers over {days} day(s): {} entries", rows.len()),
                rows,
            })
        }

        Tool::RunReadonlyQuery => {
            let sql = arg_str(args, "sql")
                .or_else(|| arg_str(args, "query"))
                .ok_or_else(|| AppError::validation("run_readonly_query needs a 'sql' field with a SELECT/WITH query"))?;
            // Defence in depth stays: validator first, then a physically read-only connection.
            // Falls back to the main connection (still validator-guarded) when no db file is
            // set, e.g. in-memory test databases.
            let safe = super::nl2sql::validate(&sql)?;
            let path = crate::db::db_path();
            if !path.as_os_str().is_empty() {
                if let Ok(conn) = crate::db::open_read_only(&path) {
                    let mut stmt = conn
                        .prepare(&safe)
                        .map_err(|e| AppError::validation(format!("could not prepare query: {e}")))?;
                    let names: Vec<String> =
                        stmt.column_names().iter().map(|s| s.to_string()).collect();
                    let col_count = names.len();
                    let mut rows_out: Vec<Value> = Vec::new();
                    let mut rows = stmt
                        .query([])
                        .map_err(|e| AppError::validation(format!("query failed: {e}")))?;
                    while let Some(row) = rows
                        .next()
                        .map_err(|e| AppError::validation(format!("row error: {e}")))?
                    {
                        if rows_out.len() >= 500 {
                            break;
                        }
                        let mut obj = serde_json::Map::with_capacity(col_count);
                        for i in 0..col_count {
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
                            obj.insert(names.get(i).cloned().unwrap_or_else(|| format!("col{i}")), v);
                        }
                        rows_out.push(Value::Object(obj));
                    }
                    return Ok(ReadResult {
                        summary: format!("Read-only query returned {} row(s)", rows_out.len()),
                        rows: rows_out,
                    });
                }
            }
            let rows_out: Vec<Value> = db.read(|c| {
                let mut stmt = c
                    .prepare(&safe)
                    .map_err(|e| AppError::validation(format!("could not prepare query: {e}")))?;
                let names: Vec<String> =
                    stmt.column_names().iter().map(|s| s.to_string()).collect();
                let col_count = names.len();
                let mut out = Vec::new();
                let mut rows = stmt
                    .query([])
                    .map_err(|e| AppError::validation(format!("query failed: {e}")))?;
                while let Some(row) = rows
                    .next()
                    .map_err(|e| AppError::validation(format!("row error: {e}")))?
                {
                    if out.len() >= 500 {
                        break;
                    }
                    let mut obj = serde_json::Map::with_capacity(col_count);
                    for i in 0..col_count {
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
                        obj.insert(names.get(i).cloned().unwrap_or_else(|| format!("col{i}")), v);
                    }
                    out.push(Value::Object(obj));
                }
                Ok::<_, AppError>(out)
            })?;
            Ok(ReadResult {
                summary: format!("Read-only query returned {} row(s)", rows_out.len()),
                rows: rows_out,
            })
        }

        other => Err(AppError::validation(format!(
            "'{}' is a write tool and cannot be used as a query",
            other.name()
        ))),
    }
}

/// Validate a write call and produce a `PendingCall`. Nothing is executed here — the caller must
/// get explicit user confirmation first.
pub fn prepare_write(db: &Db, call: &ToolCall, actor: Option<String>) -> AppResult<PendingCall> {
    let tool = Tool::parse(&call.function.name)
        .ok_or_else(|| AppError::validation(format!("unknown tool '{}'", call.function.name)))?;
    if !tool.is_write() {
        return Err(AppError::validation(format!(
            "'{}' is not a write tool",
            tool.name()
        )));
    }
    let raw = &call.function.arguments;

    match tool {
        Tool::CreateProduct => {
            let name = arg_str(raw, "name")
                .ok_or_else(|| AppError::validation("create_product needs a name"))?;
            if find_or_create_id(db, &name)?.is_some() {
                return Err(AppError::Conflict(format!(
                    "a product named '{name}' already exists — use add_batch instead"
                )));
            }
            let args = json!({
                "name": name,
                "category": arg_str(raw, "category"),
                "unit": arg_str(raw, "unit"),
                "sku": arg_str(raw, "sku"),
                "reorderPoint": arg_f64(raw, "reorder_point"),
                "leadTimeDays": arg_i64(raw, "lead_time_days"),
            });
            Ok(PendingCall {
                summary: format!("Create product \"{name}\""),
                projection: format!("Create product '{name}'."),
                args,
                tool,
            })
        }

        Tool::AddBatch => {
            let name = arg_str(raw, "name")
                .ok_or_else(|| AppError::validation("add_batch needs a product name"))?;
            let (pid, resolved_name) = resolve_product(db, raw)?;
            let batch_no = arg_str(raw, "batch_no")
                .or_else(|| arg_str(raw, "batch"))
                .ok_or_else(|| AppError::validation("add_batch needs a batch number"))?;
            let qty = arg_i64(raw, "quantity")
                .filter(|q| *q > 0)
                .ok_or_else(|| AppError::validation("add_batch needs a positive quantity"))?;
            let price = arg_f64(raw, "unit_price")
                .ok_or_else(|| AppError::validation("add_batch needs a unit price"))?;
            let expiry = arg_str(raw, "expiry_date")
                .or_else(|| arg_str(raw, "expiry"))
                .map(|e| {
                    util::parse_date(&e)
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .ok_or_else(|| AppError::validation(format!("'{e}' is not a valid date")))
                })
                .transpose()?;
            let args = json!({
                "productId": pid,
                "batchNo": batch_no,
                "quantity": qty,
                "unitPrice": price,
                "expiryDate": expiry,
                "supplier": arg_str(raw, "supplier"),
                "actor": actor,
            });
            let _ = name;
            Ok(PendingCall {
                summary: format!(
                    "Receive {qty} × {resolved_name} into batch {batch_no} at {price:.2}{}",
                    expiry.map(|e| format!(" (expires {e})")).unwrap_or_default()
                ),
                projection: format!(
                    "Received {qty} units of {resolved_name}, batch {batch_no}, at {price:.2} each."
                ),
                args,
                tool,
            })
        }

        Tool::SellStock => {
            let (pid, resolved_name) = resolve_product(db, raw)?;
            let qty = arg_i64(raw, "quantity")
                .filter(|q| *q > 0)
                .ok_or_else(|| AppError::validation("sell_stock needs a positive quantity"))?;
            let buyer = arg_str(raw, "buyer_name")
                .or_else(|| arg_str(raw, "buyer"))
                .ok_or_else(|| AppError::validation("sell_stock needs a buyer name"))?;
            let plan = domain::plan_allocation(db, pid, qty, false)?;
            if plan.shortfall > 0 {
                let have: i64 = plan.allocations.iter().map(|a| a.quantity).sum();
                return Err(AppError::Shortfall {
                    product: resolved_name,
                    requested: qty,
                    available: have,
                });
            }
            let from = plan
                .allocations
                .iter()
                .map(|a| a.batch_no.clone())
                .collect::<Vec<_>>()
                .join(", ");
            let args = json!({
                "productId": pid,
                "quantity": qty,
                "buyerName": buyer,
                "unitPriceOverride": arg_f64(raw, "unit_price_override"),
                "actor": actor,
            });
            Ok(PendingCall {
                summary: format!("Sell {qty} × {resolved_name} to {buyer} (from batch {from})"),
                projection: format!(
                    "Sold {qty} units of {resolved_name} to {buyer}."
                ),
                args,
                tool,
            })
        }

        Tool::AdjustStock => {
            let (pid, resolved_name) = resolve_product(db, raw)?;
            let delta = arg_i64(raw, "quantity_delta")
                .or_else(|| arg_i64(raw, "delta"))
                .filter(|d| *d != 0)
                .ok_or_else(|| AppError::validation("adjust_stock needs a non-zero quantity_delta"))?;
            let reason = arg_str(raw, "reason")
                .ok_or_else(|| AppError::validation("adjust_stock needs a reason"))?;
            let batch_no = arg_str(raw, "batch_no").or_else(|| arg_str(raw, "batch"));
            let args = json!({
                "productId": pid,
                "quantityDelta": delta,
                "batchNo": batch_no,
                "reason": reason,
                "actor": actor,
            });
            Ok(PendingCall {
                summary: format!(
                    "Adjust {resolved_name} by {delta:+} ({reason})",
                ),
                projection: format!(
                    "Adjusted {resolved_name} stock by {delta:+} for: {reason}."
                ),
                args,
                tool,
            })
        }

        other => Err(AppError::validation(format!(
            "'{}' is not a write tool",
            other.name()
        ))),
    }
}

/// Commit a previously-confirmed write.
pub fn commit_write(db: &Db, tool: Tool, args: &Value) -> AppResult<Execution> {
    let actor = arg_str(args, "actor");
    let source = "agent";
    match tool {
        Tool::CreateProduct => {
            let req = NewProduct {
                name: arg_str(args, "name").unwrap_or_default(),
                sku: arg_str(args, "sku"),
                barcode: arg_str(args, "barcode"),
                category: arg_str(args, "category"),
                unit: arg_str(args, "unit"),
                description: arg_str(args, "description"),
                reorder_point: arg_f64(args, "reorderPoint").or_else(|| arg_f64(args, "reorder_point")),
                lead_time_days: arg_i64(args, "leadTimeDays").or_else(|| arg_i64(args, "lead_time_days")),
            };
            let p = db.write(|c| repo::create_product(c, &req))?;
            Ok(Execution {
                summary: format!("Created product \"{}\" (id {})", p.name, p.id),
                projection: format!("Created product '{}'.", p.name),
                changed: true,
            })
        }

        Tool::AddBatch => {
            let req = RestockRequest {
                product: None,
                batch: NewBatch {
                    product_id: arg_i64(args, "productId").unwrap_or_default(),
                    batch_no: arg_str(args, "batchNo").or_else(|| arg_str(args, "batch_no")).unwrap_or_default(),
                    expiry_date: arg_str(args, "expiryDate").or_else(|| arg_str(args, "expiry_date")),
                    unit_price: arg_f64(args, "unitPrice").or_else(|| arg_f64(args, "unit_price")).unwrap_or(0.0),
                    quantity: arg_i64(args, "quantity").unwrap_or(0),
                    received_at: None,
                    supplier: arg_str(args, "supplier"),
                    notes: arg_str(args, "notes"),
                },
                source: Some(source.into()),
                actor,
                note: Some("Added via agent".into()),
            };
            let r = domain::restock(db, &req)?;
            Ok(Execution {
                summary: format!(
                    "Received {} × {} into batch {} ({} now on hand)",
                    r.quantity, r.product_name, r.allocations[0].batch_no, r.remaining_quantity
                ),
                projection: format!(
                    "Received {} units of {}. {} now on hand.",
                    r.quantity, r.product_name, r.remaining_quantity
                ),
                changed: true,
            })
        }

        Tool::SellStock => {
            let req = SaleRequest {
                product_id: arg_i64(args, "productId").unwrap_or_default(),
                quantity: arg_i64(args, "quantity").unwrap_or(0),
                buyer_name: arg_str(args, "buyerName")
                    .or_else(|| arg_str(args, "buyer_name"))
                    .unwrap_or_default(),
                unit_price_override: arg_f64(args, "unitPriceOverride")
                    .or_else(|| arg_f64(args, "unit_price_override")),
                allow_expired: false,
                note: Some("Sold via agent".into()),
                source: Some(source.into()),
                actor,
            };
            let r = domain::sell(db, &req)?;
            let batches = r
                .allocations
                .iter()
                .map(|a| format!("{}×{}", a.quantity, a.batch_no))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(Execution {
                summary: format!(
                    "Sold {} × {} to {} for {:.2} (batches: {})",
                    r.quantity, r.product_name, req.buyer_name, r.total_value, batches
                ),
                projection: format!(
                    "Sold {} units of {} to {} for {:.2}. {} remain on hand.",
                    r.quantity, r.product_name, req.buyer_name, r.total_value, r.remaining_quantity
                ),
                changed: true,
            })
        }

        Tool::AdjustStock => {
            let req = AdjustRequest {
                product_id: arg_i64(args, "productId").unwrap_or_default(),
                quantity_delta: arg_i64(args, "quantityDelta")
                    .or_else(|| arg_i64(args, "quantity_delta"))
                    .unwrap_or(0),
                batch_no: arg_str(args, "batchNo").or_else(|| arg_str(args, "batch_no")),
                reason: arg_str(args, "reason"),
                source: Some(source.into()),
                actor,
            };
            let r = domain::adjust(db, &req)?;
            Ok(Execution {
                summary: format!(
                    "Adjusted {} by {}+, now {} on hand",
                    r.product_name, r.quantity, r.remaining_quantity
                ),
                projection: format!(
                    "Adjusted {} stock by {}+. {} now on hand.",
                    r.product_name, r.quantity, r.remaining_quantity
                ),
                changed: true,
            })
        }

        other => Err(AppError::validation(format!("'{}' is not a write tool", other.name()))),
    }
}

fn find_or_create_id(db: &Db, name: &str) -> AppResult<Option<i64>> {
    db.read(|c| {
        if let Some(p) = repo::find_product_by_name(c, name)? {
            return Ok(Some(p.id));
        }
        let matches = repo::search_products(c, name, 1)?;
        Ok(matches.first().map(|m| m.id))
    })
}

/// Compact text projection handed to the model. Bounded so a large result cannot blow the
/// context window (PRD §8.3).
pub fn project(rows: &[Value], max_rows: usize) -> String {
    let mut out = String::new();
    for (i, r) in rows.iter().take(max_rows).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let pairs: Vec<String> = r
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| {
                        let val = match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        format!("{k}={val}")
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push_str(&pairs.join(" | "));
    }
    if rows.len() > max_rows {
        out.push_str(&format!("\n… and {} more rows", rows.len() - max_rows));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewBatch, NewProduct, RestockRequest};

    fn seeded_db() -> Db {
        let db = Db::open_in_memory().expect("in-memory db");
        crate::domain::restock(
            &db,
            &RestockRequest {
                product: Some(NewProduct {
                    name: "Amoxicillin".into(),
                    category: Some("Pharma".into()),
                    unit: Some("pcs".into()),
                    ..NewProduct::default()
                }),
                batch: NewBatch {
                    product_id: 0,
                    batch_no: "B1".into(),
                    expiry_date: Some("2027-03-01".into()),
                    unit_price: 12.5,
                    quantity: 100,
                    received_at: None,
                    supplier: None,
                    notes: None,
                },
                source: Some("ui".into()),
                actor: None,
                note: None,
            },
        )
        .expect("seed restock");
        db
    }

    #[test]
    fn readonly_query_reads_all_data() {
        let db = seeded_db();
        let res = run_read(
            &db,
            Tool::RunReadonlyQuery,
            &json!({"sql": "SELECT name, category FROM products LIMIT 10"}),
        )
        .expect("select should succeed");
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0]["name"], json!("Amoxicillin"));

        // Joins across all inventory tables work.
        let join = run_read(
            &db,
            Tool::RunReadonlyQuery,
            &json!({"sql": "SELECT p.name, SUM(b.quantity) q FROM products p JOIN batches b ON b.product_id = p.id GROUP BY p.id LIMIT 10"}),
        )
        .expect("join should succeed");
        assert_eq!(join.rows.len(), 1);
    }

    #[test]
    fn readonly_query_still_rejects_writes() {
        let db = seeded_db();
        for bad in [
            "DELETE FROM ledger",
            "UPDATE products SET name='x'",
            "INSERT INTO products VALUES (1)",
            "DROP TABLE products",
            "SELECT * FROM sqlite_master LIMIT 1",
        ] {
            assert!(
                run_read(&db, Tool::RunReadonlyQuery, &json!({"sql": bad})).is_err(),
                "should have rejected: {bad}"
            );
        }
    }
}