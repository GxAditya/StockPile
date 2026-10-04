import { ArrowRight, Boxes, CornerDownLeft, History, Search, ShoppingCart } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../lib/api";
import { fmtNum } from "../lib/format";
import type { Product } from "../lib/types";

interface Row {
  id: string;
  kind: "product" | "nav";
  title: string;
  subtitle: string;
  to: string;
  meta?: string;
  metaTone?: string;
  product?: Product;
}

const NAV_ROWS: Row[] = [
  { id: "n1", kind: "nav", title: "Dashboard", subtitle: "KPIs and charts", to: "/" },
  { id: "n2", kind: "nav", title: "Inventory", subtitle: "Stock by product and batch", to: "/inventory" },
  { id: "n3", kind: "nav", title: "Add stock", subtitle: "Receive a batch", to: "/add" },
  { id: "n4", kind: "nav", title: "Sell", subtitle: "Record a sale", to: "/sell" },
  { id: "n5", kind: "nav", title: "History", subtitle: "Every movement, with buyers", to: "/history" },
  { id: "n6", kind: "nav", title: "Insights", subtitle: "Deterministic analysis", to: "/insights" },
  { id: "n7", kind: "nav", title: "Settings", subtitle: "Models, data, routing", to: "/settings" },
];

/** ⌘K palette. FTS-backed product search plus static navigation. */
export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Product[]>([]);
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const nav = useNavigate();

  useEffect(() => {
    if (open) {
      setQ("");
      setHits([]);
      setCursor(0);
      // Focus after paint so the caret does not jump.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const term = q.trim();
    if (term.length < 1) {
      setHits([]);
      return;
    }
    const t = window.setTimeout(() => {
      api
        .searchProducts(term, 8)
        .then((rows) => {
          setHits(rows);
          setCursor(0);
        })
        .catch(() => setHits([]));
    }, 110);
    return () => window.clearTimeout(t);
  }, [q, open]);

  const rows = useMemo<Row[]>(() => {
    const productRows: Row[] = hits.map((p) => ({
      id: `p${p.id}`,
      kind: "product",
      title: p.name,
      subtitle: [p.sku, p.category].filter(Boolean).join(" · ") || "No SKU",
      to: `/inventory?focus=${p.id}`,
      product: p,
    }));
    const term = q.trim().toLowerCase();
    const navRows = NAV_ROWS.filter(
      (n) => !term || n.title.toLowerCase().includes(term) || n.subtitle.toLowerCase().includes(term),
    );
    return [...productRows, ...navRows];
  }, [hits, q]);

  if (!open) return null;

  const go = (r: Row) => {
    nav(r.to);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/62 p-6 pt-[14vh] backdrop-blur-[2px]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="card w-full max-w-[600px] overflow-hidden shadow-2xl shadow-black/60">
        <div className="flex items-center gap-2.5 border-b border-border px-4">
          <Search size={15.5} className="shrink-0 text-muted" strokeWidth={1.75} />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setCursor((c) => Math.min(c + 1, rows.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setCursor((c) => Math.max(c - 1, 0));
              } else if (e.key === "Enter" && rows[cursor]) {
                e.preventDefault();
                go(rows[cursor]);
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
            placeholder="Search products or jump to a page…"
            className="h-[46px] flex-1 bg-transparent text-[14px] outline-none placeholder:text-faint"
          />
          <kbd className="rounded border border-border bg-surface-2 px-1.5 py-0.5 font-mono text-[10px] text-muted">
            ESC
          </kbd>
        </div>

        <div className="max-h-[52vh] overflow-y-auto py-1.5">
          {rows.length === 0 ? (
            <div className="px-4 py-10 text-center text-[13px] text-muted">
              {q.trim() ? `No product or page matches “${q}”.` : "Start typing to search."}
            </div>
          ) : (
            rows.map((r, i) => {
              const active = i === cursor;
              const Icon = r.kind === "product" ? Boxes : r.to === "/sell" ? ShoppingCart : r.to === "/history" ? History : null;
              return (
                <button
                  key={r.id}
                  onMouseEnter={() => setCursor(i)}
                  onClick={() => go(r)}
                  className={`flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                    active ? "bg-accent/16" : ""
                  }`}
                >
                  <span className={active ? "text-ink" : "text-muted"}>
                    {Icon ? <Icon size={15.5} strokeWidth={1.75} /> : <ArrowRight size={15.5} strokeWidth={1.75} />}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium text-ink">{r.title}</span>
                    <span className="block truncate text-[11.5px] text-muted">{r.subtitle}</span>
                  </span>
                  {active ? <CornerDownLeft size={13} className="text-muted" /> : null}
                </button>
              );
            })
          )}
        </div>

        <div className="flex items-center justify-between border-t border-border px-4 py-2 text-[11px] text-faint">
          <span className="flex items-center gap-3">
            <span>↑↓ navigate</span>
            <span>⏎ open</span>
          </span>
          <span className="tnum">{fmtNum(rows.length)} result{rows.length === 1 ? "" : "s"}</span>
        </div>
      </div>
    </div>
  );
}