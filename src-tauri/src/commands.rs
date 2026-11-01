//! Tauri IPC surface. Every command returns `Result<T, AppError>`; the renderer maps the
//! stable `code` to localised copy rather than pattern-matching message strings.

use crate::agent::{self, router, tools, Agent, AgentEvent, PendingMutation};
use crate::analytics;
use crate::db::repo::{self, StockQuery};
use crate::db::Db;
use crate::domain;
use crate::error::{AppError, AppResult};
use crate::insights;
use crate::models::*;
use crate::seed;
use crate::util;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub db: Db,
    pub agent: Agent,
    /// Mutations awaiting user confirmation, keyed by call id.
    pub pending: Mutex<HashMap<String, PendingMutation>>,
}

impl AppState {
    pub fn db(&self) -> &Db {
        &self.db
    }
}

type Cmd<'a> = State<'a, AppState>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockQueryArgs {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

// ── Inventory ───────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn stock_list(state: Cmd<'_>, args: Option<StockQueryArgs>) -> AppResult<StockList> {
    let a = args.unwrap_or(StockQueryArgs {
        search: None,
        category: None,
        health: None,
        sort: None,
        limit: None,
        offset: None,
    });
    state.db().read(|c| {
        repo::stock_list(
            c,
            &StockQuery {
                search: a.search,
                category: a.category,
                health: a.health,
                sort: a.sort,
                limit: a.limit,
                offset: a.offset,
            },
        )
    })
}

#[tauri::command]
pub fn product_detail(state: Cmd<'_>, id: i64) -> AppResult<ProductDetail> {
    state.db().read(|c| repo::product_detail(c, id))
}

#[tauri::command]
pub fn create_product(state: Cmd<'_>, product: NewProduct) -> AppResult<Product> {
    state.db().write(|c| repo::create_product(c, &product))
}

#[tauri::command]
pub fn update_product(
    state: Cmd<'_>,
    id: i64,
    product: NewProduct,
) -> AppResult<Product> {
    state.db().write(|c| repo::update_product(c, id, &product))
}

#[tauri::command]
pub fn search_products(
    state: Cmd<'_>,
    query: String,
    limit: Option<i64>,
) -> AppResult<Vec<Product>> {
    state
        .db()
        .read(|c| repo::search_products(c, &query, limit.unwrap_or(10).clamp(1, 50)))
}

#[tauri::command]
pub fn categories(state: Cmd<'_>) -> AppResult<Vec<String>> {
    state.db().read(|c| repo::categories(c))
}

#[tauri::command]
pub fn restock(state: Cmd<'_>, req: RestockRequest) -> AppResult<SaleResult> {
    domain::restock(state.db(), &req)
}

/// FEFO preview. Pure read — the UI calls this before showing the confirm button.
#[tauri::command]
pub fn preview_sale(
    state: Cmd<'_>,
    product_id: i64,
    quantity: i64,
    allow_expired: Option<bool>,
) -> AppResult<SalePreview> {
    let plan = domain::plan_allocation(
        state.db(),
        product_id,
        quantity.max(0),
        allow_expired.unwrap_or(false),
    )?;
    let total: f64 = plan
        .allocations
        .iter()
        .map(|a| a.quantity as f64 * a.unit_price)
        .sum();
    Ok(SalePreview {
        allocations: plan.allocations,
        shortfall: plan.shortfall,
        total_value: (total * 100.0).round() / 100.0,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalePreview {
    pub allocations: Vec<SaleAllocation>,
    pub shortfall: i64,
    pub total_value: f64,
}

#[tauri::command]
pub fn sell(state: Cmd<'_>, req: SaleRequest) -> AppResult<SaleResult> {
    domain::sell(state.db(), &req)
}

#[tauri::command]
pub fn adjust_stock(state: Cmd<'_>, req: AdjustRequest) -> AppResult<SaleResult> {
    domain::adjust(state.db(), &req)
}

// ── History ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn history(state: Cmd<'_>, filter: Option<HistoryFilter>) -> AppResult<HistoryPage> {
    let f = filter.unwrap_or(HistoryFilter {
        kinds: None,
        product_id: None,
        search: None,
        from: None,
        to: None,
        buyer_name: None,
        limit: Some(200),
        offset: None,
    });
    state.db().read(|c| repo::history(c, &f))
}

#[tauri::command]
pub fn export_csv(
    state: Cmd<'_>,
    filter: Option<HistoryFilter>,
) -> AppResult<ExportResult> {
    let f = filter.unwrap_or(HistoryFilter {
        kinds: None,
        product_id: None,
        search: None,
        from: None,
        to: None,
        buyer_name: None,
        limit: None,
        offset: None,
    });
    let rows = state.db().read(|c| repo::history_flat(c, &f))?;

    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record([
        "id", "timestamp", "kind", "product", "batch_no", "expiry_date", "qty_delta",
        "unit_price", "total_value", "buyer_name", "source", "actor", "note",
    ])?;
    for r in &rows {
        wtr.write_record([
            r.id.to_string(),
            r.ts.clone(),
            r.kind.clone(),
            r.product_name.clone(),
            r.batch_no.clone().unwrap_or_default(),
            r.expiry_date.clone().unwrap_or_default(),
            r.qty_delta.to_string(),
            format!("{:.2}", r.unit_price),
            format!("{:.2}", r.total_value),
            r.buyer_name.clone().unwrap_or_default(),
            r.source.clone(),
            r.actor.clone().unwrap_or_default(),
            r.note.clone().unwrap_or_default(),
        ])?;
    }
    let bytes = wtr
        .into_inner()
        .map_err(|e| AppError::internal(format!("csv: {e}")))?;

    Ok(ExportResult {
        filename: format!("stockpile-history-{}.csv", util::today_str()),
        path: None,
        content: String::from_utf8_lossy(&bytes).to_string(),
        rows: rows.len(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub filename: String,
    pub path: Option<String>,
    pub content: String,
    pub rows: usize,
}

#[tauri::command]
pub fn export_inventory_csv(state: Cmd<'_>, args: Option<StockQueryArgs>) -> AppResult<ExportResult> {
    let a = args.unwrap_or(StockQueryArgs {
        search: None,
        category: None,
        health: None,
        sort: None,
        limit: None,
        offset: None,
    });
    let list = state.db().read(|c| {
        repo::stock_list(
            c,
            &StockQuery {
                search: a.search,
                category: a.category,
                health: a.health,
                sort: a.sort,
                limit: Some(5000),
                offset: None,
            },
        )
    })?;

    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record([
        "product", "sku", "category", "unit", "on_hand", "batches", "avg_cost", "total_value",
        "earliest_expiry", "health",
    ])?;
    for r in &list.rows {
        wtr.write_record([
            r.name.clone(),
            r.sku.clone().unwrap_or_default(),
            r.category.clone().unwrap_or_default(),
            r.unit.clone(),
            r.total_quantity.to_string(),
            r.batch_count.to_string(),
            format!("{:.2}", r.avg_cost),
            format!("{:.2}", r.total_value),
            r.earliest_expiry.clone().unwrap_or_default(),
            r.health.as_str().to_string(),
        ])?;
    }
    let bytes = wtr
        .into_inner()
        .map_err(|e| AppError::internal(format!("csv: {e}")))?;

    Ok(ExportResult {
        filename: format!("stockpile-inventory-{}.csv", util::today_str()),
        path: None,
        content: String::from_utf8_lossy(&bytes).to_string(),
        rows: list.rows.len(),
    })
}

// ── Dashboard & insights ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn dashboard(state: Cmd<'_>) -> AppResult<analytics::Dashboard> {
    analytics::dashboard(state.db())
}

#[tauri::command]
pub fn insights(state: Cmd<'_>) -> AppResult<insights::Insights> {
    insights::all(state.db())
}

// ── Agent ───────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealth {
    pub online: bool,
    pub host: String,
    pub models: Vec<AgentModelInfo>,
    pub loaded: Vec<LoadedModelInfo>,
    pub routing: Vec<RouteInfo>,
    pub overrides: Vec<(String, String)>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelInfo {
    pub name: String,
    pub size_gb: f64,
    pub parameter_size: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModelInfo {
    pub name: String,
    pub size_gb: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    pub task: String,
    pub model: String,
    pub keep_alive: String,
    pub approx_size: String,
}

#[tauri::command]
pub async fn agent_health(state: Cmd<'_>) -> AppResult<AgentHealth> {
    let online = state.agent.ollama.health().await.is_ok();
    let (models, loaded) = if online {
        (
            state
                .agent
                .ollama
                .list_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| AgentModelInfo {
                    parameter_size: m.details.as_ref().and_then(|d| d.parameter_size.clone()),
                    name: m.name,
                    size_gb: (m.size as f64 / 1e9 * 100.0).round() / 100.0,
                })
                .collect(),
            state
                .agent
                .ollama
                .running_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| LoadedModelInfo {
                    name: m.name,
                    size_gb: (m.size_vram as f64 / 1e9 * 100.0).round() / 100.0,
                })
                .collect(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(AgentHealth {
        online,
        host: state.agent.ollama.host().to_string(),
        models,
        loaded,
        routing: router::default_table()
            .into_iter()
            .map(|(task, model, keep_alive, approx_size)| RouteInfo {
                task,
                model,
                keep_alive,
                approx_size,
            })
            .collect(),
        overrides: router::overrides()
            .into_iter()
            .map(|(t, m)| (t, m))
            .collect(),
        degraded: !online,
    })
}

#[tauri::command]
pub async fn agent_unload_models(state: Cmd<'_>) -> AppResult<()> {
    state.agent.ollama.unload_all().await
}

#[tauri::command]
pub fn agent_set_override(task: String, model: Option<String>) -> AppResult<()> {
    let kind = match task.as_str() {
        "classify" => router::TaskKind::Classify,
        "extract" => router::TaskKind::Extract,
        "resolve" => router::TaskKind::Resolve,
        "narrate" => router::TaskKind::Narrate,
        "reason" => router::TaskKind::Reason,
        other => return Err(AppError::validation(format!("unknown task '{other}'"))),
    };
    router::set_override(kind, model);
    Ok(())
}

/// One agent turn. Events are emitted on `agent://event` as they are produced so the UI can
/// stream text and render tool cards incrementally.
#[tauri::command]
pub async fn agent_chat(
    app: AppHandle,
    state: Cmd<'_>,
    message: String,
    session_id: Option<String>,
) -> AppResult<()> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(AppError::validation("message is empty"));
    }
    if message.chars().count() > 4000 {
        return Err(AppError::validation("message is too long"));
    }
    let session_id = session_id.unwrap_or_else(|| "default".into());

    let db_path = crate::db::db_path();
    // Collect only the mutations proposed by *this* turn. Pre-existing entries stay in
    // `state.pending` untouched so an earlier confirmation card remains confirmable even
    // after the operator sends a follow-up message.
    let mut new_pending: Vec<PendingMutation> = Vec::new();

    let agent = state.agent.clone();
    let events = agent
        .handle(state.db(), &db_path, &session_id, &message, &mut new_pending)
        .await;

    // Register newly proposed mutations under the same ids sent to the renderer in the
    // `Confirmation` events, so a later `agent_confirm` can resolve them.
    {
        let mut guard = state.pending.lock();
        for p in new_pending {
            guard.insert(p.call_id.clone(), p);
        }
    }

    for ev in events {
        emit(&app, &session_id, ev);
    }
    Ok(())
}

fn emit(app: &AppHandle, session_id: &str, event: AgentEvent) {
    let _ = app.emit(
        "agent://event",
        serde_json::json!({ "sessionId": session_id, "event": event }),
    );
}

#[tauri::command]
pub fn agent_confirm(
    state: Cmd<'_>,
    call_id: String,
) -> AppResult<CommittedWrite> {
    let pending = state.pending.lock().remove(&call_id);
    let p = pending.ok_or_else(|| AppError::NotFound("that confirmation is no longer pending".into()))?;
    let exec = tools::commit_write(state.db(), p.tool, &p.args)?;
    Ok(CommittedWrite {
        tool: p.tool.name().to_string(),
        summary: exec.summary,
        changed: exec.changed,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommittedWrite {
    pub tool: String,
    pub summary: String,
    pub changed: bool,
}

#[tauri::command]
pub fn agent_cancel(state: Cmd<'_>, call_id: String) -> AppResult<bool> {
    Ok(state.pending.lock().remove(&call_id).is_some())
}

#[tauri::command]
pub fn agent_stop(state: Cmd<'_>) {
    state
        .agent
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub fn agent_clear_history(state: Cmd<'_>, session_id: Option<String>) -> AppResult<()> {
    agent::clear_memory(state.db(), &session_id.unwrap_or_else(|| "default".into()))
}

#[tauri::command]
pub fn agent_history(state: Cmd<'_>, session_id: Option<String>) -> AppResult<Vec<AgentMessage>> {
    let session_id = session_id.unwrap_or_else(|| "default".into());
    state.db().read(|c| {
        let mut st = c.prepare(
            "SELECT id, role, content, ts FROM agent_messages WHERE session_id = ?1 ORDER BY id ASC LIMIT 100",
        )?;
        let rows = st
            .query_map([&session_id], |r| {
                Ok(AgentMessage {
                    id: r.get(0)?,
                    role: r.get(1)?,
                    content: r.get(2)?,
                    ts: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub ts: String,
}

/// Resolve a single product reference the way the agent does, for the inline typeahead.
#[tauri::command]
pub fn resolve_product(state: Cmd<'_>, query: String) -> AppResult<ProductMatch> {
    let db = state.db();
    let (id, candidates) = agent::intent::resolve(db, Some(&query))?;
    let rows = db.read(|c| repo::search_products(c, &query, 6))?;
    Ok(ProductMatch {
        product_id: id,
        candidates,
        products: rows,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductMatch {
    pub product_id: Option<i64>,
    pub candidates: Vec<String>,
    pub products: Vec<Product>,
}

// ── Lifecycle ────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn seed_demo(state: Cmd<'_>) -> AppResult<seed::SeedReport> {
    seed::seed(state.db())
}

#[tauri::command]
pub fn app_info(state: Cmd<'_>) -> AppResult<AppInfo> {
    let db = state.db();
    let counts = db.read(|c| {
        let p: i64 = c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        let b: i64 = c.query_row("SELECT COUNT(*) FROM batches", [], |r| r.get(0))?;
        let l: i64 = c.query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))?;
        Ok::<_, AppError>((p, b, l))
    })?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: crate::db::db_path().to_string_lossy().to_string(),
        fts5: db.caps().fts5,
        products: counts.0,
        batches: counts.1,
        ledger_rows: counts.2,
        today: util::today_str(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub db_path: String,
    pub fts5: bool,
    pub products: i64,
    pub batches: i64,
    pub ledger_rows: i64,
    pub today: String,
}

/// Remove every record. Used by the "reset demo data" control in Settings.
#[tauri::command]
pub fn reset_database(state: Cmd<'_>) -> AppResult<()> {
    let fts5 = state.db().caps().fts5;
    state.db().write(|c| {
        c.execute("DELETE FROM ledger", [])?;
        c.execute("DELETE FROM batches", [])?;
        c.execute("DELETE FROM products", [])?;
        c.execute("DELETE FROM usage_daily", [])?;
        c.execute("DELETE FROM agent_messages", [])?;
        if fts5 {
            let _ = c.execute("INSERT INTO products_fts(products_fts) VALUES('rebuild')", []);
        }
        Ok(())
    })
}

pub fn default_db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("STOCKPILE_DB") {
        if !p.trim().is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    dirs_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.stockpile.app")
        .join("stockpile.db")
}

fn dirs_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(x) = std::env::var("XDG_DATA_HOME") {
            if !x.trim().is_empty() {
                return Some(std::path::PathBuf::from(x));
            }
        }
        std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .map(|h| std::path::PathBuf::from(h).join(".local/share"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(std::path::PathBuf::from)
    }
}

pub fn build_state(db: Db) -> AppResult<AppState> {
    Ok(AppState {
        db,
        agent: Agent::new(),
        pending: Mutex::new(HashMap::new()),
    })
}