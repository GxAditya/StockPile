import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import type {
  AdjustRequest,
  AgentEvent,
  AgentHealth,
  AgentMessage,
  AppError,
  AppInfo,
  CommittedWrite,
  Dashboard,
  ExportResult,
  HistoryFilter,
  HistoryPage,
  Insights,
  NewProduct,
  Product,
  ProductDetail,
  ProductMatch,
  RestockRequest,
  SalePreview,
  SaleRequest,
  SaleResult,
  SeedReport,
  StockList,
  StockQueryArgs,
} from "./types";

/**
 * Tauri throws a rejected promise whose value is the serialised `AppError`. Normalise it into a
 * typed error so callers can branch on `code` instead of pattern-matching message text.
 */
export class ApiError extends Error {
  readonly code: string;
  readonly details: Record<string, unknown> | null;

  constructor(code: string, message: string, details: Record<string, unknown> | null = null) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.details = details;
  }

  get shortfallBy(): number | null {
    const v = this.details?.shortBy;
    return typeof v === "number" ? v : null;
  }
}

async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  try {
    return (await invoke(cmd, args)) as T;
  } catch (e) {
    const err = e as Partial<AppError> & { message?: string };
    throw new ApiError(
      err?.code ?? "UNKNOWN",
      err?.message ?? (typeof e === "string" ? e : "Something went wrong"),
      err?.details ?? null,
    );
  }
}

export const api = {
  // ── Inventory ──────────────────────────────────────────────────────────────
  stockList: (args: StockQueryArgs = {}) => call<StockList>("stock_list", { args }),
  productDetail: (id: number) => call<ProductDetail>("product_detail", { id }),
  createProduct: (product: NewProduct) => call<Product>("create_product", { product }),
  updateProduct: (id: number, product: NewProduct) =>
    call<Product>("update_product", { id, product }),
  searchProducts: (query: string, limit?: number) =>
    call<Product[]>("search_products", { query, limit }),
  categories: () => call<string[]>("categories"),
  restock: (req: RestockRequest) => call<SaleResult>("restock", { req }),
  previewSale: (productId: number, quantity: number, allowExpired?: boolean) =>
    call<SalePreview>("preview_sale", { productId, quantity, allowExpired: allowExpired ?? false }),
  sell: (req: SaleRequest) => call<SaleResult>("sell", { req }),
  adjustStock: (req: AdjustRequest) => call<SaleResult>("adjust_stock", { req }),

  // ── History ────────────────────────────────────────────────────────────────
  history: (filter: HistoryFilter = {}) => call<HistoryPage>("history", { filter }),
  exportCsv: (filter: HistoryFilter = {}) => call<ExportResult>("export_csv", { filter }),
  exportInventoryCsv: (args: StockQueryArgs = {}) =>
    call<ExportResult>("export_inventory_csv", { args }),

  // ── Dashboard & insights ───────────────────────────────────────────────────
  dashboard: () => call<Dashboard>("dashboard"),
  insights: () => call<Insights>("insights"),

  // ── Agent ──────────────────────────────────────────────────────────────────
  agentHealth: () => call<AgentHealth>("agent_health"),
  agentChat: (message: string, sessionId: string) =>
    call<void>("agent_chat", { message, sessionId }),
  agentConfirm: (callId: string) => call<CommittedWrite>("agent_confirm", { callId }),
  agentCancel: (callId: string) => call<boolean>("agent_cancel", { callId }),
  agentStop: () => call<void>("agent_stop"),
  agentClearHistory: (sessionId: string) => call<void>("agent_clear_history", { sessionId }),
  agentHistory: (sessionId: string) => call<AgentMessage[]>("agent_history", { sessionId }),
  agentSetOverride: (task: string, model: string | null) =>
    call<void>("agent_set_override", { task, model }),
  agentUnloadModels: () => call<void>("agent_unload_models"),
  resolveProduct: (query: string) => call<ProductMatch>("resolve_product", { query }),

  // ── Lifecycle ──────────────────────────────────────────────────────────────
  seedDemo: () => call<SeedReport>("seed_demo"),
  appInfo: () => call<AppInfo>("app_info"),
  resetDatabase: () => call<void>("reset_database"),
};

type Unlisten = () => void;

/** Subscribe to the agent stream. Returns an unsubscribe function. */
export function onAgentEvent(
  handler: (e: { sessionId: string; event: AgentEvent }) => void,
): Promise<Unlisten> {
  return listen<{ sessionId: string; event: AgentEvent }>("agent://event", (e) =>
    handler(e.payload),
  );
}

/** Save a text file through the OS dialog. Returns the chosen path, or null if cancelled. */
export async function saveTextFile(
  suggestedName: string,
  content: string,
): Promise<string | null> {
  const path = await save({
    defaultPath: suggestedName,
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!path) return null;
  await writeTextFile(path, content);
  return path;
}