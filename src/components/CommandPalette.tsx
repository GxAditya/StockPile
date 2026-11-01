import { ArrowRight, Boxes, CornerDownLeft, History, Search, ShoppingCart } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../lib/api";
import { haptic } from "../lib/motion";
import { fmtNum } from "../lib/format";
import type { Product } from "../lib/types";
import { Spinner } from "./ui";

interface Row {
  id: string;
  kind: "product" | "nav";
  title: string;
  subtitle: string;
  to: string;
  product?: Product;
}

const NAV_ROWS: Row[] = [
  { id: "n1", kind: "nav", title: "Dashboard", subtitle: "KPIs and charts", to: "/" },
  { id: "n2", kind: "nav", title: "Inventory", subtitle: "Stock by product and batch", to: "/inventory" },
  { id: "n3", kind: "nav", title: "Add stock", subtitle: "Receive a batch", to: "/add" },
  { id: "n4", kind: "nav", title: "Sell", subtitle: "Record a sale", to: "/sell" },
  { id: "n5", kind: "nav", title: "History", subtitle: "Every movement, with buyers", to: "/history" },
  { id: "n6", kind: "nav", title: "Insights", subtitle: "Deterministic analysis", to: "/insights" },
  { id: "n7", kind: "nav", title: "Settings", subtitle: "Appearance, models, data", to: "/settings" },
];

const NAV_ICON: Record<string, typeof Boxes> = {
  "/sell": ShoppingCart,
  "/history": History,
};

/**
 * ⌘K palette: full-text product search plus static navigation.
 *
 * The highlight follows the pointer and the arrow keys continuously, so the
 * user always knows what Enter will open — never only at the moment it commits.
 */
export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Product[]>([]);
  const [searching, setSearching] = useState(false);
  const [cursor, setCursor] = useState(0);
  const [visible, setVisible] = useState(open);
  const [closing, setClosing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const nav = useNavigate();

  useEffect(() => {
    if (open) {
      setVisible(true);
      setClosing(false);
      setQ("");
      setHits([]);
      setSearching(false);
      setCursor(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    } else if (visible) {
      setClosing(true);
      const t = window.setTimeout(() => {
        setVisible(false);
        setClosing(false);
      }, 200);
      return () => window.clearTimeout(t);
    }
    return undefined;
  }, [open, visible]);

  useEffect(() => {
    if (!visible || closing) return undefined;
    const term = q.trim();
    if (term.length < 1) {
      setHits([]);
      setSearching(false);
      return undefined;
    }
    setSearching(true);
    const t = window.setTimeout(() => {
      api
        .searchProducts(term, 8)
        .then((rows) => {
          setHits(rows);
          setCursor(0);
        })
        .catch(() => setHits([]))
        .finally(() => setSearching(false));
    }, 110);
    return () => window.clearTimeout(t);
  }, [q, visible, closing]);

  const rows = useMemo<Row[]>(() => {
    const productRows: Row[] = hits.map((p) => ({
      id: `p${p.id}`,
      kind: "product",
      title: p.name,
      // Show what there is, then abbreviate an id rather than dumping a UUID:
      // the tail is what distinguishes two similar SKUs.
      subtitle: [p.sku && shortSku(p.sku), p.category].filter(Boolean).join(" · ") || "No SKU",
      to: `/inventory?focus=${p.id}`,
      product: p,
    }));
    const term = q.trim().toLowerCase();
    const navRows = NAV_ROWS.filter(
      (n) => !term || n.title.toLowerCase().includes(term) || n.subtitle.toLowerCase().includes(term),
    );
    return [...productRows, ...navRows];
  }, [hits, q]);

  // Keep the active row in view as the cursor moves.
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${cursor}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  if (!visible) return null;

  const go = (r: Row) => {
    haptic("tick");
    nav(r.to);
    onClose();
  };

  return (
    <div
      className={`scrim fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] p-6 pt-[12vh] ${
        closing ? "is-closing" : ""
      }`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`glass-pop w-full max-w-[560px] overflow-hidden rounded-[var(--radius-pop)] ${
          closing ? "is-closing material-pop" : "material-pop"
        }`}
        role="dialog"
        aria-modal="true"
        aria-label="Search products or jump to a page"
      >
        <div className="flex items-center gap-2.5 px-4">
          <Search size={15} className="shrink-0 text-faint" strokeWidth={1.9} aria-hidden />
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
                go(rows[cursor]!);
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
            placeholder="Search products or jump to a page…"
            className="h-[44px] flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-faint"
            aria-label="Search products or pages"
            aria-expanded={rows.length > 0}
            aria-activedescendant={rows[cursor]?.id}
          />
          {searching ? <Spinner size={12} className="text-faint" /> : null}
          <kbd className="kbd">esc</kbd>
        </div>

        <div
          ref={listRef}
          className="scroll-y max-h-[52vh] border-t border-[var(--hairline)] py-1.5"
          role="listbox"
        >
          {rows.length === 0 ? (
            <div className="px-4 py-10 text-center">
              <p className="text-[12.5px] text-muted">
                {q.trim() ? `No product or page matches “${q}”.` : "Start typing to search."}
              </p>
              <p className="mt-1 text-[11px] text-faint">
                Products come from full-text search · pages filter as you type
              </p>
            </div>
          ) : (
            rows.map((r, i) => {
              const active = i === cursor;
              const Icon = r.kind === "product" ? Boxes : NAV_ICON[r.to];
              return (
                <button
                  key={r.id}
                  id={r.id}
                  data-idx={i}
                  role="option"
                  aria-selected={active}
                  onMouseEnter={() => setCursor(i)}
                  onFocus={() => setCursor(i)}
                  onClick={() => go(r)}
                  className={`flex w-full items-center gap-3 px-4 py-[7px] text-left transition-colors ${
                    active ? "bg-accent/15" : ""
                  }`}
                >
                  <span className={active ? "text-accent" : "text-faint"} aria-hidden>
                    {Icon ? (
                      <Icon size={15} strokeWidth={1.9} />
                    ) : (
                      <ArrowRight size={15} strokeWidth={1.9} />
                    )}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px] font-[510] text-ink">
                      {r.title}
                    </span>
                    <span className="block truncate text-[11px] text-muted">{r.subtitle}</span>
                  </span>
                  {active ? (
                    <CornerDownLeft size={12.5} className="shrink-0 text-muted" aria-hidden />
                  ) : null}
                </button>
              );
            })
          )}
        </div>

        <div className="flex items-center justify-between border-t border-[var(--hairline)] px-4 py-2 text-[11px] text-faint">
          <span className="flex items-center gap-3">
            <span className="flex items-center gap-1">
              <kbd className="kbd">↑</kbd>
              <kbd className="kbd">↓</kbd> navigate
            </span>
            <span className="flex items-center gap-1">
              <kbd className="kbd">⏎</kbd> open
            </span>
          </span>
          <span className="tnum">
            {fmtNum(rows.length)} result{rows.length === 1 ? "" : "s"}
          </span>
        </div>
      </div>
    </div>
  );
}

/** Long SKUs are noise in a 40-char subtitle; keep the distinguishing tail. */
function shortSku(sku: string): string {
  return sku.length > 14 ? `…${sku.slice(-12)}` : sku;
}
