use serde::{Deserialize, Serialize};

/// ISO-8601 date, `YYYY-MM-DD`.
pub type DateStr = String;
/// ISO-8601 timestamp, RFC3339-ish UTC.
pub type Timestamp = String;

pub const DEFAULT_UNIT: &str = "pcs";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub category: Option<String>,
    pub unit: String,
    pub description: Option<String>,
    pub reorder_point: f64,
    pub lead_time_days: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: i64,
    pub product_id: i64,
    pub batch_no: String,
    pub expiry_date: Option<DateStr>,
    pub unit_price: f64,
    pub quantity: i64,
    pub received_at: DateStr,
    pub supplier: Option<String>,
    pub notes: Option<String>,
    /// Denormalised for display; `None` when the parent product was deleted.
    pub product_name: Option<String>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LedgerKind {
    Restock,
    Sale,
    Adjustment,
    Void,
}

impl LedgerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerKind::Restock => "restock",
            LedgerKind::Sale => "sale",
            LedgerKind::Adjustment => "adjustment",
            LedgerKind::Void => "void",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "restock" => Some(LedgerKind::Restock),
            "sale" => Some(LedgerKind::Sale),
            "adjustment" => Some(LedgerKind::Adjustment),
            "void" => Some(LedgerKind::Void),
            _ => None,
        }
    }

    /// Inflow vs outflow, used by the dashboard flow chart and sign normalisation.
    pub fn direction(self) -> i64 {
        match self {
            LedgerKind::Restock => 1,
            LedgerKind::Sale => -1,
            LedgerKind::Adjustment => 0,
            LedgerKind::Void => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub id: i64,
    pub ts: Timestamp,
    pub kind: String,
    pub product_id: i64,
    pub product_name: String,
    pub batch_id: Option<i64>,
    pub batch_no: Option<String>,
    pub expiry_date: Option<DateStr>,
    pub qty_delta: i64,
    pub unit_price: f64,
    pub total_value: f64,
    pub buyer_name: Option<String>,
    pub note: Option<String>,
    pub source: String,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StockHealth {
    Ok,
    Low,
    Out,
    Expiring,
    Expired,
}

impl StockHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            StockHealth::Ok => "ok",
            StockHealth::Low => "low",
            StockHealth::Out => "out",
            StockHealth::Expiring => "expiring",
            StockHealth::Expired => "expired",
        }
    }
}

/// A product row in the inventory view: product identity plus computed stock aggregates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockRow {
    pub id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub category: Option<String>,
    pub unit: String,
    pub description: Option<String>,
    pub reorder_point: f64,
    pub lead_time_days: i64,
    pub total_quantity: i64,
    pub batch_count: i64,
    pub avg_cost: f64,
    pub total_value: f64,
    pub earliest_expiry: Option<DateStr>,
    pub expired_units: i64,
    pub expiring_units: i64,
    pub health: StockHealth,
    pub last_movement_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockList {
    pub rows: Vec<StockRow>,
    pub total: usize,
    pub grand_total_units: i64,
    pub grand_total_value: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProduct {
    pub name: String,
    #[serde(default)]
    pub sku: Option<String>,
    #[serde(default)]
    pub barcode: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub reorder_point: Option<f64>,
    #[serde(default)]
    pub lead_time_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewBatch {
    pub product_id: i64,
    pub batch_no: String,
    #[serde(default)]
    pub expiry_date: Option<DateStr>,
    pub unit_price: f64,
    pub quantity: i64,
    #[serde(default)]
    pub received_at: Option<DateStr>,
    #[serde(default)]
    pub supplier: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestockRequest {
    /// When present and non-empty, the product is created if it does not exist.
    #[serde(default)]
    pub product: Option<NewProduct>,
    pub batch: NewBatch,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleRequest {
    pub product_id: i64,
    pub quantity: i64,
    pub buyer_name: String,
    #[serde(default)]
    pub unit_price_override: Option<f64>,
    /// Permit drawing from expired batches. Off by default.
    #[serde(default)]
    pub allow_expired: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleAllocation {
    pub batch_id: i64,
    pub batch_no: String,
    pub expiry_date: Option<DateStr>,
    pub unit_price: f64,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleResult {
    pub product_id: i64,
    pub product_name: String,
    pub quantity: i64,
    pub total_value: f64,
    pub allocations: Vec<SaleAllocation>,
    pub remaining_quantity: i64,
    pub ledger_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustRequest {
    pub product_id: i64,
    pub quantity_delta: i64,
    #[serde(default)]
    pub batch_no: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFilter {
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    #[serde(default)]
    pub product_id: Option<i64>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub from: Option<DateStr>,
    #[serde(default)]
    pub to: Option<DateStr>,
    #[serde(default)]
    pub buyer_name: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDay {
    pub date: DateStr,
    pub entries: Vec<LedgerEntry>,
    pub units_in: i64,
    pub units_out: i64,
    pub value_in: f64,
    pub value_out: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub days: Vec<HistoryDay>,
    pub total_rows: i64,
    pub units_in: i64,
    pub units_out: i64,
    pub value_in: f64,
    pub value_out: f64,
    pub buyers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDetail {
    pub product: Product,
    pub stock: StockRow,
    pub batches: Vec<Batch>,
}