import { useQuery } from "@tanstack/react-query";
import { Search, ShoppingCart } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { fmtMoney, fmtNum, expiryTone, HEALTH_META, TONE_TEXT } from "../lib/format";
import type { StockRow } from "../lib/types";
import { SellModal } from "../components/SellModal";
import { EmptyState, SkeletonTable } from "../components/ui";

const SELL_COLUMNS = [
  { label: "Product" },
  { label: "Category" },
  { label: "Earliest expiry" },
  { label: "On hand", num: true },
  { label: "Batches", num: true },
  { label: "Avg price", num: true },
  { label: "Status" },
  { label: "", width: 86 },
];

/**
 * Standalone selling surface: pick a product, then the FEFO-aware sale dialog
 * runs exactly as it does from the inventory table. One implementation of a
 * sale, reached from two places.
 */
export function Sell() {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<StockRow | null>(null);

  const list = useQuery({
    queryKey: ["stock", "", "all", "all", "totalQuantity"],
    queryFn: () => api.stockList({ sort: "totalQuantity", limit: 1000 }),
  });

  const term = query.trim().toLowerCase();
  const rows = (list.data?.rows ?? []).filter((r) => {
    if (r.totalQuantity < 1) return false;
    if (!term) return true;
    return (
      r.name.toLowerCase().includes(term) ||
      (r.sku ?? "").toLowerCase().includes(term) ||
      (r.category ?? "").toLowerCase().includes(term)
    );
  });

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="toolbar-glass shrink-0 border-b border-[var(--hairline)] px-5 py-2.5">
        <div className="relative max-w-md">
          <Search
            size={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint"
            strokeWidth={1.9}
          />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find a product to sell…"
            className="input !pl-8"
            aria-label="Search products to sell"
          />
        </div>
        <p className="mt-2 text-[11.5px] text-muted">
          Only products with stock are listed. Sales allocate earliest-expiry batches first.
        </p>
      </div>

      <div className="scroll-y min-h-0 flex-1">
        {list.isLoading ? (
          <SkeletonTable columns={SELL_COLUMNS} rows={10} />
        ) : rows.length === 0 ? (
          <EmptyState
            icon={<ShoppingCart size={19} strokeWidth={1.9} />}
            title={term ? "Nothing sellable matches" : "No stock available to sell"}
            body={
              term
                ? "Try a different name, or clear the search to see everything in stock."
                : "Every product is out of stock. Receive stock first, then come back here."
            }
          />
        ) : (
          <table className="tbl tbl-rows-interactive">
            <thead>
              <tr>
                {SELL_COLUMNS.map((c, i) => (
                  <th key={i} className={c.num ? "num" : ""} style={c.width ? { width: c.width } : undefined}>
                    {c.label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const tone = expiryTone(r.earliestExpiry);
                const meta = HEALTH_META[r.health];
                return (
                  <tr key={r.id}>
                    <td>
                      <span className="font-[510] text-ink">{r.name}</span>
                      {r.sku ? (
                        <span className="ml-2 font-mono text-[10.5px] text-faint">{r.sku}</span>
                      ) : null}
                    </td>
                    <td className="text-[12px]">{r.category ?? "—"}</td>
                    <td className={`tnum text-[12px] ${TONE_TEXT[tone.tone]}`}>
                      {r.earliestExpiry ? `${r.earliestExpiry} · ${tone.label}` : "No expiry"}
                    </td>
                    <td className="num tnum font-[510] text-ink">
                      {fmtNum(r.totalQuantity)}
                      <span className="ml-1 text-[10.5px] text-faint">{r.unit}</span>
                    </td>
                    <td className="num tnum">{r.batchCount}</td>
                    <td className="num tnum">{fmtMoney(r.avgCost, true)}</td>
                    <td>
                      <span className={`pill ${meta.tone}`}>
                        <span aria-hidden>{meta.glyph}</span>
                        {meta.label}
                      </span>
                    </td>
                    <td>
                      <div className="flex justify-end">
                        <button
                          onClick={() => setSelected(r)}
                          className="btn btn-ghost !h-[25px] !px-2.5 !text-[11.5px]"
                        >
                          Sell
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      <SellModal
        row={selected}
        onClose={() => setSelected(null)}
        onDone={() => setSelected(null)}
      />
    </div>
  );
}
