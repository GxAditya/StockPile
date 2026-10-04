import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, PackagePlus, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { errorCopy, useApp } from "../lib/ctx";
import { fmtMoney, fmtNum } from "../lib/format";
import type { Product } from "../lib/types";
import { Field, Spinner } from "./ui";

const UNITS = ["pcs", "box", "strip", "bottle", "kg", "g", "litre", "pack", "vial"];
const todayStr = () => new Date().toISOString().slice(0, 10);

/**
 * The receive form itself, without the modal chrome so it can live on a page.
 *
 * One form covers both cases: an existing product (add a batch) or a brand-new product (create
 * it and add its first batch in one transaction). This is the same single implementation the
 * old dialog used — there is only one way to receive stock.
 */
export function AddStockForm({
  mode,
  onModeChange,
  onDone,
  defaultProductId,
}: {
  mode: "existing" | "new";
  onModeChange: (m: "existing" | "new") => void;
  onDone: () => void;
  defaultProductId?: number;
}) {
  const { toast } = useApp();
  const qc = useQueryClient();

  const [productId, setProductId] = useState<number | null>(defaultProductId ?? null);
  const [query, setQuery] = useState("");
  const [touched, setTouched] = useState(false);

  // New-product fields
  const [name, setName] = useState("");
  const [category, setCategory] = useState("");
  const [unit, setUnit] = useState("pcs");
  const [reorderPoint, setReorderPoint] = useState("0");

  // Batch fields
  const [batchNo, setBatchNo] = useState("");
  const [expiry, setExpiry] = useState("");
  const [price, setPrice] = useState("");
  const [qty, setQty] = useState("");
  const [supplier, setSupplier] = useState("");
  const [error, setError] = useState<{ title: string; body: string } | null>(null);

  const search = useQuery({
    queryKey: ["addstock-search", query],
    queryFn: () => api.searchProducts(query, 8),
    enabled: mode === "existing" && query.trim().length > 0,
  });

  // Batch fields belong to the batch, not the product, so they survive switching which product
  // is receiving. Only the product selection resets.
  useEffect(() => {
    setProductId(defaultProductId ?? null);
    setTouched(false);
    setError(null);
  }, [defaultProductId]);

  const selected: Product | null = useMemo(() => {
    if (productId === null) return null;
    const list = search.data ?? [];
    return list.find((p) => p.id === productId) ?? null;
  }, [productId, search.data]);

  const pickExisting = (p: Product) => {
    setProductId(p.id);
    setQuery(p.name);
    setTouched(false);
    setError(null);
  };

  const submit = useMutation({
    mutationFn: async () => {
      if (mode === "existing") {
        if (productId === null) throw new Error("Choose a product first");
        return api.restock({
          product: null,
          batch: {
            productId,
            batchNo: batchNo.trim(),
            expiryDate: expiry || null,
            unitPrice: Number(price),
            quantity: Number(qty),
            receivedAt: todayStr(),
            supplier: supplier.trim() || null,
          },
          source: "ui",
        });
      }
      return api.restock({
        product: {
          name: name.trim(),
          category: category.trim() || null,
          unit,
          reorderPoint: Number(reorderPoint) || 0,
          leadTimeDays: 7,
        },
        batch: {
          productId: 0,
          batchNo: batchNo.trim(),
          expiryDate: expiry || null,
          unitPrice: Number(price),
          quantity: Number(qty),
          receivedAt: todayStr(),
          supplier: supplier.trim() || null,
        },
        source: "ui",
      });
    },
    onSuccess: (r) => {
      toast(
        "ok",
        `Received ${fmtNum(r.quantity)} ${selected?.unit ?? unit}`,
        `${r.productName} — batch ${r.allocations[0]?.batchNo ?? batchNo}, now ${fmtNum(r.remainingQuantity)} on hand`,
      );
      setBatchNo("");
      setExpiry("");
      setPrice("");
      setQty("");
      setSupplier("");
      setError(null);
      void qc.invalidateQueries();
      onDone();
    },
    onError: (e) => setError(errorCopy(e)),
  });

  const canSubmit = (() => {
    if (mode === "existing" && productId === null) return false;
    if (mode === "new" && name.trim().length < 2) return false;
    if (batchNo.trim().length === 0) return false;
    const q = Number(qty);
    const p = Number(price);
    return Number.isFinite(q) && q >= 1 && Number.isFinite(p) && p >= 0;
  })();

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSubmit) {
      e.preventDefault();
      submit.mutate();
    }
  };

  const lineTotal =
    Number.isFinite(Number(price)) && Number(qty) > 0
      ? (Number(price) || 0) * (Number(qty) || 0)
      : null;

  return (
    <div onKeyDown={onKeyDown} className="space-y-4">
      {/* ── Product target ────────────────────────────────────────────────── */}
      <div>
        <div className="mb-2 flex gap-1.5">
          <ModeTab active={mode === "existing"} onClick={() => onModeChange("existing")}>
            Existing product
          </ModeTab>
          <ModeTab active={mode === "new"} onClick={() => onModeChange("new")}>
            New product
          </ModeTab>
        </div>

        {mode === "existing" ? (
          <div className="relative">
            <Search
              size={14}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint"
              strokeWidth={1.75}
            />
            <input
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setTouched(true);
              }}
              placeholder="Type a product name or SKU…"
              className="input !pl-8"
              aria-label="Product search"
            />
            {touched && (search.data?.length ?? 0) > 0 ? (
              <ul className="absolute z-20 mt-1 max-h-[210px] w-full overflow-y-auto rounded-[10px] border border-border-strong bg-surface-2 py-1 shadow-2xl shadow-black/50">
                {search.data?.map((p) => (
                  <li key={p.id}>
                    <button
                      onClick={() => pickExisting(p)}
                      className={`flex w-full items-center justify-between gap-3 px-3 py-2 text-left transition-colors hover:bg-surface-3 ${
                        productId === p.id ? "bg-accent/16" : ""
                      }`}
                    >
                      <span className="min-w-0">
                        <span className="block truncate text-[13px] text-ink">{p.name}</span>
                        <span className="block truncate text-[11px] text-faint">
                          {[p.sku, p.category, p.unit].filter(Boolean).join(" · ")}
                        </span>
                      </span>
                      {productId === p.id ? (
                        <Check size={14} className="shrink-0 text-accent-soft" />
                      ) : null}
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
            {touched && query.trim() && (search.data?.length ?? 0) === 0 && !search.isFetching ? (
              <div className="mt-1.5 flex items-center justify-between gap-2 rounded-lg border border-border bg-surface-2 px-3 py-2 text-[11.5px] text-muted">
                <span className="min-w-0 truncate">No product matches “{query.trim()}”.</span>
                <button
                  onClick={() => {
                    onModeChange("new");
                    setName(query.trim());
                  }}
                  className="btn btn-ghost !h-[24px] shrink-0 !px-2 !text-[11px]"
                >
                  Create it
                </button>
              </div>
            ) : null}
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            <div className="col-span-2">
              <Field label="Product name" required>
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g. Amoxicillin 500mg Capsules"
                  className="input"
                />
              </Field>
            </div>
            <Field label="Category">
              <input
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                placeholder="Pharma"
                className="input"
              />
            </Field>
            <Field label="Unit">
              <select value={unit} onChange={(e) => setUnit(e.target.value)} className="input">
                {UNITS.map((u) => (
                  <option key={u} value={u}>
                    {u}
                  </option>
                ))}
              </select>
            </Field>
            <div className="col-span-2">
              <Field label="Reorder point" hint="Low-stock threshold in units">
                <input
                  value={reorderPoint}
                  onChange={(e) => setReorderPoint(e.target.value)}
                  inputMode="numeric"
                  className="input tnum"
                />
              </Field>
            </div>
          </div>
        )}
      </div>

      <div className="h-px bg-border" />

      {/* ── Batch details ─────────────────────────────────────────────────── */}
      <div>
        <h3 className="mb-2.5 text-[12px] font-semibold tracking-tight text-ink">Batch details</h3>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Batch / lot number" required>
            <input
              value={batchNo}
              onChange={(e) => setBatchNo(e.target.value)}
              placeholder="B2401"
              className="input font-mono"
            />
          </Field>
          <Field label="Expiry date" hint="Leave blank for non-perishables">
            <input
              type="date"
              value={expiry}
              onChange={(e) => setExpiry(e.target.value)}
              className="input tnum"
            />
          </Field>
          <Field label="Unit price" required>
            <input
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              inputMode="decimal"
              placeholder="12.50"
              className="input tnum"
            />
          </Field>
          <Field label="Quantity" required>
            <input
              value={qty}
              onChange={(e) => setQty(e.target.value)}
              inputMode="numeric"
              placeholder="40"
              className="input tnum"
            />
          </Field>
          <div className="col-span-2">
            <Field label="Supplier" hint="Optional">
              <input
                value={supplier}
                onChange={(e) => setSupplier(e.target.value)}
                placeholder="Optional"
                className="input"
              />
            </Field>
          </div>
        </div>
        {lineTotal !== null ? (
          <div className="mt-3 flex items-baseline justify-between rounded-lg border border-border bg-surface-2 px-3 py-2">
            <span className="text-[11.5px] text-muted">Line total</span>
            <span className="tnum text-[15px] font-semibold text-ink">
              {fmtMoney(lineTotal, true)}
            </span>
          </div>
        ) : null}
      </div>

      {selected ? (
        <p className="rounded-lg border border-border bg-surface-2 px-3 py-2 text-[11.5px] leading-snug text-muted">
          Adding a batch to <span className="text-ink">{selected.name}</span>. If batch{" "}
          <span className="font-mono text-ink">{batchNo || "…"}</span> already exists its
          quantity is topped up and the unit cost becomes a weighted average.
        </p>
      ) : null}

      {error ? (
        <p className="rounded-lg border border-danger/35 bg-danger/8 px-3 py-2 text-[11.5px] leading-snug text-danger">
          <span className="font-medium">{error.title}.</span> {error.body}
        </p>
      ) : null}

      <button
        onClick={() => submit.mutate()}
        disabled={!canSubmit || submit.isPending}
        className="btn btn-primary w-full !h-[34px]"
      >
        {submit.isPending ? (
          <Spinner size={14} />
        ) : (
          <PackagePlus size={14.5} strokeWidth={1.75} />
        )}
        Receive stock
      </button>
      <p className="text-center text-[11px] text-faint">
        {canSubmit ? "⌘⏎ to receive" : "Batch number, quantity and price are required"}
      </p>
    </div>
  );
}

function ModeTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-lg px-3 py-1.5 text-[12.5px] font-medium transition-colors ${
        active
          ? "bg-accent/18 text-ink shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--color-accent)_45%,transparent)]"
          : "text-muted hover:bg-surface-2 hover:text-ink-2"
      }`}
    >
      {children}
    </button>
  );
}
