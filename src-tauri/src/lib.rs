pub mod agent;
pub mod analytics;
pub mod commands;
pub mod db;
pub mod domain;
pub mod error;
pub mod insights;
pub mod models;
pub mod seed;
pub mod util;

use commands::AppState;
use error::AppError;
use tauri::Manager;

/// WebKitGTK's render pipeline needs help on some hardware/drivers. These must be set before the
/// event loop starts so the web content process inherits them. Existing values win, so an operator
/// can always override from the environment.
fn configure_webview_env() {
    for (key, value) in [
        // The DMABUF renderer aborts with SIGTRAP on this machine's Intel Iris Xe GT2, killing
        // the web content process so the window painted a flat unpainted grey. The software
        // path is slower but correct.
        //
        // This is the *only* flag needed. An earlier attempt also set WEBKIT_FORCE_SANDBOX=0,
        // which modern WebKitGTK ignores (it warns and does nothing), so it bought nothing and
        // would have weakened isolation if it had worked. Not set.
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
    ] {
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

pub fn run() {
    configure_webview_env();

    let db_path = commands::default_db_path();
    let db = db::Db::open(&db_path).unwrap_or_else(|e| {
        eprintln!("fatal: cannot open database at {db_path:?}: {e}");
        std::process::exit(1);
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::build_state(db).expect("state"))
        .invoke_handler(tauri::generate_handler![
            commands::stock_list,
            commands::product_detail,
            commands::create_product,
            commands::update_product,
            commands::search_products,
            commands::categories,
            commands::restock,
            commands::preview_sale,
            commands::sell,
            commands::adjust_stock,
            commands::history,
            commands::export_csv,
            commands::export_inventory_csv,
            commands::dashboard,
            commands::insights,
            commands::agent_health,
            commands::agent_chat,
            commands::agent_confirm,
            commands::agent_cancel,
            commands::agent_stop,
            commands::agent_clear_history,
            commands::agent_history,
            commands::agent_set_override,
            commands::agent_unload_models,
            commands::resolve_product,
            commands::seed_demo,
            commands::app_info,
            commands::reset_database,
        ])
        .on_window_event(|window, event| {
            // Evict models when the window closes so a quit app holds no model resident (AC-8).
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    let agent = state.agent.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = agent.ollama.unload_all().await;
                    });
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running StockPile");
}

/// Helper for tests: open a fresh in-memory database with the full schema applied.
pub fn test_db() -> Result<db::Db, AppError> {
    db::Db::open_in_memory()
}