import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Layers,
  PackagePlus,
  Search,
  SlidersHorizontal,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { api, saveTextFile } from "../lib/api";
import { useApp } from "../lib/ctx";
import {
  fmtDate,
  fmtMoney,
  fmtNum,
  expiryTone,
  HEALTH_META,
  HEALTH_ORDER,
} from "../lib/format";
import type { Batch, Health, StockRow } from "../lib/types";
import { AddStockModal } from "../components/AddStockModal";
import { SellModal } from "../components/SellModal";
import { EmptyState, ErrorState, SkeletonRows } from "../components/ui";

type SortKey =
  | "name"
  | "category"
  | "totalQuantity"
  | "totalValue"
  | "batchCount"
  | "avgCost"
  | "earliestExpiry"
  | "health";

const COLUMNS: { key: SortKey; label: string; num?: boolean }[] = [
  { key: "name", label: "Product" },
  { key: "category", label: "Category" },
  { key: "totalQuantity", label: "On hand", num: true },
  { key: "batchCount", label: "Batches", num: true },
  { key: "avgCost", label: "Avg cost", num: true },
  { key: "totalValue", label: "Stock value", num: true },
  { key: "earliestExpiry", label: "Earliest expiry" },
  { key: "health", label: "Status" },
];

export function Inventory() {
  const { toast, reportError } = useApp();
  const qc = useQueryClient();
  const [params] = useSearchParams();

  const [search, setSearch] = useState(params.get("q") ?? "");
  const [debounced, setDebounced] = useState(search);
  const [category, setCategory] = useState<string>("all");
  const [health, setHealth] = useState<string>("all");
  const [sort, setSort] = useState<SortKey>("name");
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [addOpen, setAddOpen] = useState(false);
  const [sellFor, setSellFor] = useState<StockRow | null>(null);

  const focusId = params.get("focus");

  useEffect(() => {
    const t = window.setTimeout(() => setDebounced(search), 180);
    return () => window.clearTimeout(t);
  }, [search]);

  useEffect(() => {
    if (focusId) {
      const id = Number(focusId);
      if (Number.isFinite(id)) setExpanded((s) => new Set(s).add(id));
    }
  }, [focusId]);

  const list = useQuery({
    queryKey: ["stock", debounced, category, health, sort],
    queryFn: () =>
      api.stockList({
        search: debounced.trim() || null,
        category: category === "all" ? null : category,
        health,
        sort,
        limit: 1000,
      }),
  });

  const cats = useQuery({ queryKey: ["categories"], queryFn: api.categories });

  const exportCsv = useMutation({
    mutationFn: () =>
      api.exportInventoryCsv({
        search: debounced.trim() || null,
        category: category === "all" ? null : category,
        health,
      }),
    onSuccess: async (res) => {
      const path = await saveTextFile(res.filename, res.content);
      if (path) toast("ok", `Exported ${fmtNum(res.rows)} rows`, path.split("/").pop());
    },
    onError: (e) => reportError(e, "Export failed"),
  });

  const rows = list.data?.rows ?? [];
  const grandUnits = list.data?.grandTotalUnits ?? 0;
  const grandValue = list.data?.grandTotalValue ?? 0;
  const filtered = debounced.trim() || category !== "all" || health !== "all";

  const toggle = (id: number) =>
    setExpanded((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  const onSort = (key: SortKey) => setSort(key);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* ── Toolbar ────────────────────────────────────────────────────────── */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-5 py-3">
        <div className="relative min-w-[220px] flex-1">
          <Search
            size={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint"
            strokeWidth={1.75}
          />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search name, SKU or category…"
            className="input !pl-8"
            aria-label="Search inventory"
          />
        </div>

        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          className="input w-[168px]"
          aria-label="Filter by category"
        >
          <option value="all">All categories</option>
          {(cats.data ?? []).map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>

        <select
          value={health}
          onChange={(e) => setHealth(e.target.value)}
          className="input w-[152px]"
          aria-label="Filter by health"
        >
          {HEALTH_ORDER.map((h) => (
            <option key={h.value} value={h.value}>
              {h.label}
            </option>
          ))}
        </select>

        <button
          onClick={() => setAddOpen(true)}
          className="btn btn-primary ml-auto"
          title="Receive stock into a new or existing product"
        >
          <PackagePlus size={15} strokeWidth={1.75} />
          Add stock
        </button>

        <button
          onClick={() => exportCsv.mutate()}
          disabled={exportCsv.isPending || rows.length === 0}
          className="btn btn-ghost"
          title="Export the current filtered view as CSV"
        >
          <Download size={14.5} strokeWidth={1.75} />
          CSV
        </button>
      </div>

      {/* ── Summary strip ──────────────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-4 border-b border-border bg-surface/40 px-5 py-2 text-[11.5px]">
        <span className="flex items-center gap-1.5 text-muted">
          <Layers size={12.5} strokeWidth={1.75} />
          <span className="tnum font-medium text-ink">{fmtNum(list.data?.total ?? 0)}</span>
          product{(list.data?.total ?? 0) === 1 ? "" : "s"}
        </span>
        <span className="text-muted tnum">
          <span className="font-medium text-ink">{fmtNum(grandUnits)}</span> units
        </span>
        <span className="text-muted tnum">
          <span className="font-medium text-ink">{fmtMoney(grandValue)}</span> value
        </span>
        {filtered ? (
          <span className="flex items-center gap-1.5 text-accent-soft">
            <SlidersHorizontal size={12.5} strokeWidth={1.75} />
            filtered
          </span>
        ) : null}
        <div className="flex-1" />
        <span className="text-faint">Click a row to see its batches</span>
      </div>

      {/* ── Table ──────────────────────────────────────────────────────────── */}
      <div className="scroll-y min-h-0 flex-1">
        {list.isError ? (
          <ErrorState message={(list.error as Error).message} onRetry={() => void list.refetch()} />
        ) : list.isLoading ? (
          <table className="tbl">
            <thead>
              <tr>
                {COLUMNS.map((c) => (
                  <th key={c.key} className={c.num ? "num" : ""}>
                    {c.label}
                  </th>
                ))}
              </tr>
            </thead>
            <SkeletonRows rows={12} cols={8} />
          </table>
        ) : rows.length === 0 ? (
          filtered ? (
            <EmptyState
              icon={<Search size={19} />}
              title="No products match"
              body="Nothing in the catalogue matches those filters. Clear them, or add the product as new stock."
              action={
                <div className="flex gap-2">
                  <button
                    onClick={() => {
                      setSearch("");
                      setCategory("all");
                      setHealth("all");
                    }}
                    className="btn btn-ghost"
                  >
                    Clear filters
                  </button>
                  <button onClick={() => setAddOpen(true)} className="btn btn-primary">
                    Add stock
                  </button>
                </div>
              }
            />
          ) : (
            <EmptyState
              icon={<PackagePlus size={19} />}
              title="No stock yet"
              body="Receive your first batch to start tracking quantities, expiry dates and prices."
              action={
                <button onClick={() => setAddOpen(true)} className="btn btn-primary">
                  <PackagePlus size={15} strokeWidth={1.75} />
                  Add stock
                </button>
              }
            />
          )
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                {COLUMNS.map((c) => (
                  <th
                    key={c.key}
                    className={`${c.num ? "num" : ""} cursor-pointer select-none hover:text-ink`}
                    onClick={() => onSort(c.key)}
                    aria-sort={sort === c.key ? "ascending" : "none"}
                  >
                    <span className="inline-flex items-center gap-1">
                      {c.label}
                      {sort === c.key ? (
                        <ChevronDown
                          size={11}
                          className={c.key === "name" || c.key === "category" ? "rotate-180" : ""}
                        />
                      ) : null}
                    </span>
                  </th>
                ))}
                <th className="w-[92px]" />
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <StockRowGroup
                  key={r.id}
                  row={r}
                  open={expanded.has(r.id)}
                  highlighted={focusId === String(r.id)}
                  onToggle={() => toggle(r.id)}
                  onSell={() => setSellFor(r)}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>

      <AddStockModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onDone={() => {
          setAddOpen(false);
          void qc.invalidateQueries();
        }}
      />
      <SellModal
        row={sellFor}
        onClose={() => setSellFor(null)}
        onDone={() => {
          setSellFor(null);
          void qc.invalidateQueries();
        }}
      />
    </div>
  );
}

function StockRowGroup({
  row,
  open,
  highlighted,
  onToggle,
  onSell,
}: {
  row: StockRow;
  open: boolean;
  highlighted: boolean;
  onToggle: () => void;
  onSell: () => void;
}) {
  const tone = expiryTone(row.earliestExpiry);
  const meta = HEALTH_META[row.health as Health];

  return (
    <>
      <tr
        onClick={onToggle}
        className={`cursor-pointer ${highlighted ? "!bg-accent/14" : ""}`}
        aria-expanded={open}
      >
        <td>
          <span className="flex items-center gap-1.5">
            {open ? (
              <ChevronDown size={13} className="shrink-0 text-faint" />
            ) : (
              <ChevronRight size={13} className="shrink-0 text-faint" />
            )}
            <span className="font-medium text-ink">{row.name}</span>
            {row.sku ? <span className="font-mono text-[10.5px] text-faint">{row.sku}</span> : null}
          </span>
        </td>
        <td className="text-[12px]">{row.category ?? "—"}</td>
        <td className="num tnum font-medium text-ink">
          {fmtNum(row.totalQuantity)}
          <span className="ml-1 text-[10.5px] text-faint">{row.unit}</span>
        </td>
        <td className="num tnum">{row.batchCount}</td>
        <td className="num tnum">{fmtMoney(row.avgCost, true)}</td>
        <td className="num tnum font-medium text-ink">{fmtMoney(row.totalValue)}</td>
        <td>
          {row.earliestExpiry ? (
            <span className={`text-[12px] tnum ${toneClass(tone.tone)}`}>
              {fmtDate(row.earliestExpiry)}
              <span className="ml-1.5 text-[10.5px] opacity-80">{tone.label}</span>
            </span>
          ) : (
            <span className="text-[12px] text-faint">No expiry</span>
          )}
        </td>
        <td>
          <span className={`pill ${meta.tone}`}>
            <span aria-hidden>{meta.glyph}</span>
            {meta.label}
          </span>
        </td>
        <td onClick={(e) => e.stopPropagation()}>
          <div className="flex justify-end">
            <button
              onClick={onSell}
              disabled={row.totalQuantity < 1}
              className="btn btn-ghost !h-[27px] !px-2.5 !text-[12px]"
              title={row.totalQuantity < 1 ? "No stock to sell" : `Sell ${row.name}`}
            >
              Sell
            </button>
          </div>
        </td>
      </tr>
      {open ? <BatchRows productId={row.id} /> : null}
    </>
  );
}

/** Batch detail is fetched lazily on expand so the table stays light at catalogue scale. */
function BatchRows({ productId }: { productId: number }) {
  const qc = useQueryClient();
  const { toast, reportError } = useApp();
  const q = useQuery({
    queryKey: ["product", productId],
    queryFn: () => api.productDetail(productId),
    staleTime: 30_000,
  });

  const adjust = useMutation({
    mutationFn: (v: { batchNo: string; delta: number }) =>
      api.adjustStock({
        productId,
        quantityDelta: v.delta,
        batchNo: v.batchNo,
        reason: "Manual correction from batch list",
        source: "ui",
      }),
    onSuccess: (r) => {
      toast("ok", "Stock adjusted", `${r.productName}: now ${fmtNum(r.remainingQuantity)} on hand`);
      void qc.invalidateQueries({ queryKey: ["product", productId] });
      void qc.invalidateQueries({ queryKey: ["stock"] });
      void qc.invalidateQueries({ queryKey: ["dashboard"] });
      void qc.invalidateQueries({ queryKey: ["history"] });
    },
    onError: (e) => reportError(e, "Adjustment failed"),
  });

  if (q.isLoading) {
    return (
      <tr className="!bg-canvas/40">
        <td colSpan={9} className="!px-6 !py-4">
          <div className="skeleton h-3 w-40" />
        </td>
      </tr>
    );
  }
  if (q.isError || !q.data) {
    return (
      <tr className="!bg-canvas/40">
        <td colSpan={9} className="!px-6 !py-4 text-[12px] text-danger">
          Could not load batches.
        </td>
      </tr>
    );
  }

  const batches = q.data.batches;
  const live = batches.filter((b) => b.quantity > 0);

  return (
    <tr className="!bg-canvas/35">
      <td colSpan={9} className="!p-0">
        <div className="border-y border-border/70 px-6 py-3">
          <div className="mb-2 flex items-center gap-2 text-[11px] uppercase tracking-wider text-faint">
            <Layers size={11.5} strokeWidth={1.75} />
            {live.length} live batch{live.length === 1 ? "" : "es"}
            <span className="normal-case tracking-normal text-faint">
              · FEFO drains earliest expiry first
            </span>
          </div>
          <table className="tbl !text-[12px]">
            <thead>
              <tr>
                <th className="!py-1.5 !pl-0">Batch no</th>
                <th className="!py-1.5">Expiry</th>
                <th className="!py-1.5 num">Unit price</th>
                <th className="!py-1.5 num">Received qty</th>
                <th className="!py-1.5 num">Remaining</th>
                <th className="!py-1.5">Received</th>
                <th className="!py-1.5">Supplier</th>
                <th className="!py-1.5 !pr-0" />
              </tr>
            </thead>
            <tbody>
              {batches.map((b) => (
                <BatchRow
                  key={b.id}
                  batch={b}
                  onAdjust={(delta) => adjust.mutate({ batchNo: b.batchNo, delta })}
                  busy={adjust.isPending}
                />
              ))}
            </tbody>
          </table>
        </div>
      </td>
    </tr>
  );
}

function BatchRow({
  batch,
  onAdjust,
  busy,
}: {
  batch: Batch;
  onAdjust: (delta: number) => void;
  busy: boolean;
}) {
  const [confirming, setConfirming] = useState(false);
  const tone = expiryTone(batch.expiryDate);
  const dead = batch.quantity === 0;

  return (
    <tr className={dead ? "opacity-45" : ""}>
      <td className="!py-1.5 !pl-0 font-mono text-[11.5px] text-ink">{batch.batchNo}</td>
      <td className="!py-1.5">
        <span className={`tnum text-[11.5px] ${toneClass(tone.tone)}`}>{fmtDate(batch.expiryDate)}</span>
        {batch.expiryDate ? (
          <span className="ml-1.5 text-[10.5px] text-faint">{tone.label}</span>
        ) : null}
      </td>
      <td className="!py-1.5 num tnum">{fmtMoney(batch.unitPrice, true)}</td>
      <td className="!py-1.5 num tnum text-faint">—</td>
      <td className="!py-1.5 num tnum font-medium text-ink">{fmtNum(batch.quantity)}</td>
      <td className="!py-1.5 tnum text-[11.5px] text-muted">{fmtDate(batch.receivedAt)}</td>
      <td className="!py-1.5 text-[11.5px] text-muted">{batch.supplier ?? "—"}</td>
      <td className="!py-1.5 !pr-0 text-right">
        {dead ? (
          <span className="text-[10.5px] text-faint">depleted</span>
        ) : confirming ? (
          <span className="inline-flex items-center gap-1">
            <button
              onClick={() => {
                onAdjust(-batch.quantity);
                setConfirming(false);
              }}
              disabled={busy}
              className="btn btn-danger !h-[23px] !px-2 !text-[11px]"
            >
              Clear all
            </button>
            <button
              onClick={() => setConfirming(false)}
              className="btn btn-quiet !h-[23px] !px-1.5 !text-[11px]"
            >
              Cancel
            </button>
          </span>
        ) : (
          <button
            onClick={() => setConfirming(true)}
            disabled={busy}
            className="btn btn-quiet !h-[23px] !px-1.5 !text-[11px]"
            title="Adjust this batch quantity"
          >
            Adjust
          </button>
        )}
      </td>
    </tr>
  );
}

function toneClass(tone: string): string {
  return tone === "danger" ? "text-danger" : tone === "warn" ? "text-warn" : tone === "info" ? "text-info" : "text-muted";
}