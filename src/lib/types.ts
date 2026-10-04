// Mirrors the Rust types in src-tauri/src/models.rs, analytics.rs and insights.rs.
// Anything the renderer must not get wrong lives here as an explicit type.

export type Health = "ok" | "low" | "out" | "expiring" | "expired";
export type LedgerKind = "restock" | "sale" | "adjustment" | "void";
export type Severity = "critical" | "warning" | "info";

export interface Product {
  id: number;
  name: string;
  sku: string | null;
  barcode: string | null;
  category: string | null;
  unit: string;
  description: string | null;
  reorderPoint: number;
  leadTimeDays: number;
  createdAt: string;
  updatedAt: string;
}

export interface NewProduct {
  name: string;
  sku?: string | null;
  barcode?: string | null;
  category?: string | null;
  unit?: string | null;
  description?: string | null;
  reorderPoint?: number | null;
  leadTimeDays?: number | null;
}

export interface Batch {
  id: number;
  productId: number;
  batchNo: string;
  expiryDate: string | null;
  unitPrice: number;
  quantity: number;
  receivedAt: string;
  supplier: string | null;
  notes: string | null;
  productName: string | null;
  unit: string | null;
}

export interface NewBatch {
  productId: number;
  batchNo: string;
  expiryDate?: string | null;
  unitPrice: number;
  quantity: number;
  receivedAt?: string | null;
  supplier?: string | null;
  notes?: string | null;
}

export interface StockRow {
  id: number;
  name: string;
  sku: string | null;
  barcode: string | null;
  category: string | null;
  unit: string;
  description: string | null;
  reorderPoint: number;
  leadTimeDays: number;
  totalQuantity: number;
  batchCount: number;
  avgCost: number;
  totalValue: number;
  earliestExpiry: string | null;
  expiredUnits: number;
  expiringUnits: number;
  health: Health;
  lastMovementAt: string | null;
}

export interface StockList {
  rows: StockRow[];
  total: number;
  grandTotalUnits: number;
  grandTotalValue: number;
}

export interface ProductDetail {
  product: Product;
  stock: StockRow;
  batches: Batch[];
}

export interface LedgerEntry {
  id: number;
  ts: string;
  kind: string;
  productId: number;
  productName: string;
  batchId: number | null;
  batchNo: string | null;
  expiryDate: string | null;
  qtyDelta: number;
  unitPrice: number;
  totalValue: number;
  buyerName: string | null;
  note: string | null;
  source: string;
  actor: string | null;
}

export interface HistoryDay {
  date: string;
  entries: LedgerEntry[];
  unitsIn: number;
  unitsOut: number;
  valueIn: number;
  valueOut: number;
}

export interface HistoryPage {
  days: HistoryDay[];
  totalRows: number;
  unitsIn: number;
  unitsOut: number;
  valueIn: number;
  valueOut: number;
  buyers: string[];
}

export interface HistoryFilter {
  kinds?: string[] | null;
  productId?: number | null;
  search?: string | null;
  from?: string | null;
  to?: string | null;
  buyerName?: string | null;
  limit?: number | null;
  offset?: number | null;
}

export interface SaleAllocation {
  batchId: number;
  batchNo: string;
  expiryDate: string | null;
  unitPrice: number;
  quantity: number;
}

export interface SaleResult {
  productId: number;
  productName: string;
  quantity: number;
  totalValue: number;
  allocations: SaleAllocation[];
  remainingQuantity: number;
  ledgerIds: number[];
}

export interface SalePreview {
  allocations: SaleAllocation[];
  shortfall: number;
  totalValue: number;
}

export interface RestockRequest {
  product?: NewProduct | null;
  batch: NewBatch;
  source?: string | null;
  actor?: string | null;
  note?: string | null;
}

export interface SaleRequest {
  productId: number;
  quantity: number;
  buyerName: string;
  unitPriceOverride?: number | null;
  allowExpired?: boolean;
  note?: string | null;
  source?: string | null;
  actor?: string | null;
}

export interface AdjustRequest {
  productId: number;
  quantityDelta: number;
  batchNo?: string | null;
  reason?: string | null;
  source?: string | null;
  actor?: string | null;
}

export interface ExportResult {
  filename: string;
  path: string | null;
  content: string;
  rows: number;
}

export interface StockQueryArgs {
  search?: string | null;
  category?: string | null;
  health?: string | null;
  sort?: string | null;
  limit?: number | null;
  offset?: number | null;
}

export interface Kpis {
  totalUnits: number;
  totalValue: number;
  productCount: number;
  liveBatchCount: number;
  lowStockCount: number;
  outOfStockCount: number;
  expiring7dBatches: number;
  expiring30dBatches: number;
  expiredBatches: number;
  revenue30d: number;
  unitsSold30d: number;
  unitsAdded30d: number;
  distinctBuyers30d: number;
}

export interface TopProduct {
  productId: number;
  name: string;
  units: number;
  value: number;
}

export interface HorizonBucket {
  bucket: string;
  batches: number;
  units: number;
  value: number;
}

export interface AttentionItem {
  batchId: number;
  productId: number;
  productName: string;
  batchNo: string;
  expiryDate: string;
  daysRemaining: number;
  quantity: number;
  unitPrice: number;
  valueAtRisk: number;
  severity: string;
}

export interface Dashboard {
  kpis: Kpis;
  topSold30d: TopProduct[];
  expiryHorizon: HorizonBucket[];
  needsAttention: AttentionItem[];
  generatedAt: string;
}

export interface InsightItem {
  productId: number;
  productName: string;
  batchNo: string | null;
  quantity: number | null;
  value: number | null;
  days: number | null;
  suggestion: string | null;
}

export interface Insight {
  id: string;
  title: string;
  rule: string;
  severity: Severity;
  detail: string;
  metric: number | null;
  metricLabel: string | null;
  items: InsightItem[];
  itemCount: number;
}

export interface Insights {
  generatedAt: string;
  totalFlagged: number;
  insights: Insight[];
}

export interface AgentModelInfo {
  name: string;
  sizeGb: number;
  parameterSize: string | null;
}

export interface LoadedModelInfo {
  name: string;
  sizeGb: number;
}

export interface RouteInfo {
  task: string;
  model: string;
  keepAlive: string;
  approxSize: string;
}

export interface AgentHealth {
  online: boolean;
  host: string;
  models: AgentModelInfo[];
  loaded: LoadedModelInfo[];
  routing: RouteInfo[];
  overrides: [string, string][];
  degraded: boolean;
}

export interface AgentMessage {
  id: number;
  role: string;
  content: string;
  ts: string;
}

export interface ProductMatch {
  product_id: number | null;
  candidates: string[];
  products: Product[];
}

export interface SeedReport {
  products: number;
  batches: number;
  sales: number;
}

export interface AppInfo {
  version: string;
  dbPath: string;
  fts5: boolean;
  products: number;
  batches: number;
  ledgerRows: number;
  today: string;
}

export interface CommittedWrite {
  tool: string;
  summary: string;
  changed: boolean;
}

/** Agent stream events, matching `agent::AgentEvent`. */
export type AgentEvent =
  | { kind: "text"; delta: string }
  | {
      kind: "toolCall";
      tool: string;
      args: unknown;
      summary: string;
      rows?: Record<string, unknown>[] | null;
    }
  | { kind: "confirmation"; callId: string; tool: string; summary: string; args: unknown }
  | { kind: "committed"; tool: string; summary: string }
  | { kind: "route"; model: string; task: string; keepAlive: string; path: string }
  | { kind: "done"; text: string; changed: boolean }
  | { kind: "error"; message: string }
  | { kind: "notice"; message: string };

/** The renderer never mutates inventory. Every error carries a stable code it can switch on. */
export interface AppError {
  code: string;
  message: string;
  details: Record<string, unknown> | null;
}