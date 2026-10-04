import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ShoppingCart } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { errorCopy, useApp } from "../lib/ctx";
import { fmtDate, fmtMoney, fmtNum, expiryTone } from "../lib/format";
import type { StockRow } from "../lib/types";
import { Field, Modal, Spinner } from "./ui";

/**
 * Sell stock. The FEFO plan is previewed before commit so the operator sees exactly which
 * batches will be drawn and what it will cost, and a shortfall blocks the sale outright.
 */
export function SellModal({
  row,
  onClose,
  onDone,
}: {
  row: StockRow | null;
  onClose: () => void;
  onDone: () => void;
}) {
  const { toast } = useApp();
  const qc = useQueryClient();

  const [buyer, setBuyer] = useState("");
  const [qty, setQty] = useState("1");
  const [allowExpired, setAllowExpired] = useState(false);
  const [priceOverride, setPriceOverride] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<{ title: string; body: string } | null>(null);
  const [touched, setTouched] = useState(false);

  useEffect(() => {
    if (row) {
      setBuyer("");
      setQty("1");
      setAllowExpired(false);
      setPriceOverride("");
      setNote("");
      setError(null);
      setTouched(false);
    }
  }, [row]);

  const quantity = Number(qty);
  const valid = Number.isFinite(quantity) && quantity >= 1 && buyer.trim().length >= 2;

  // Live FEFO preview. Debounced so typing a quantity does not spam the database.
  const preview = useQuery({
    queryKey: ["preview-sale", row?.id, quantity, allowExpired],
    queryFn: () => api.previewSale(row!.id, quantity, allowExpired),
    enabled: !!row && Number.isFinite(quantity) && quantity >= 1,
  });

  const submit = useMutation({
    mutationFn: () =>
      api.sell({
        productId: row!.id,
        quantity,
        buyerName: buyer.trim(),
        unitPriceOverride: priceOverride.trim() ? Number(priceOverride) : null,
        allowExpired,
        note: note.trim() || null,
        source: "ui",
      }),
    onSuccess: (r) => {
      toast(
        "ok",
        `Sold ${fmtNum(r.quantity)} to ${buyer.trim()}`,
        `${r.productName} — ${fmtMoney(r.totalValue, true)}, ${fmtNum(r.remainingQuantity)} still on hand`,
      );
      void qc.invalidateQueries();
      onDone();
    },
    onError: (e) => setError(errorCopy(e)),
  });

  if (!row) return null;

  const plan = preview.data;
  const shortfall = plan?.shortfall ?? 0;
  const hasExpired = (plan?.allocations ?? []).some((a) => {
    const t = expiryTone(a.expiryDate);
    return t.tone === "danger" && (a.expiryDate ?? "") <= new Date().toISOString().slice(0, 10);
  });

  return (
    <Modal
      open
      onClose={onClose}
      title="Sell stock"
      subtitle={row.name}
      width={560}
      footer={
        <>
          {error ? (
            <span className="mr-auto max-w-[260px] truncate text-[11.5px] text-danger">
              {error.title}: {error.body}
            </span>
          ) : null}
          <button onClick={onClose} className="btn btn-ghost">
            Cancel
          </button>
          <button
            onClick={() => submit.mutate()}
            disabled={!valid || shortfall > 0 || submit.isPending || preview.isLoading}
            className="btn btn-primary"
          >
            {submit.isPending ? <Spinner size={14} /> : <ShoppingCart size={14.5} strokeWidth={1.75} />}
            Record sale
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="grid grid-cols-2 gap-3">
          <div className="col-span-2">
            <Field
              label="Buyer"
              required
              error={touched && buyer.trim().length < 2 ? "Enter the buyer's name" : null}
            >
              <input
                value={buyer}
                onChange={(e) => setBuyer(e.target.value)}
                onBlur={() => setTouched(true)}
                placeholder="Mr. Rajesh Sharma"
                className="input"
                aria-invalid={touched && buyer.trim().length < 2}
              />
            </Field>
          </div>

          <Field
            label={`Quantity (${row.unit})`}
            required
            hint={`${fmtNum(row.totalQuantity)} available`}
            error={
              touched && (!Number.isFinite(quantity) || quantity < 1)
                ? "Enter a whole number of units"
                : null
            }
          >
            <input
              value={qty}
              onChange={(e) => setQty(e.target.value)}
              onBlur={() => setTouched(true)}
              inputMode="numeric"
              className="input tnum"
              aria-invalid={touched && (!Number.isFinite(quantity) || quantity < 1)}
            />
          </Field>

          <Field label="Unit price override" hint={plan?.allocations[0] ? `Default ${fmtMoney(plan.allocations[0].unitPrice, true)}` : "Blank uses batch price"}>
            <input
              value={priceOverride}
              onChange={(e) => setPriceOverride(e.target.value)}
              inputMode="decimal"
              placeholder="—"
              className="input tnum"
            />
          </Field>

          <div className="col-span-2">
            <Field label="Note">
              <input
                value={note}
                onChange={(e) => setNote(e.target.value)}
                placeholder="Optional"
                className="input"
              />
            </Field>
          </div>
        </div>

        {/* ── FEFO allocation preview ────────────────────────────────────── */}
        <div className="rounded-[10px] border border-border bg-surface-2/60">
          <header className="flex items-center justify-between border-b border-border px-3.5 py-2">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-muted">
              FEFO allocation
            </span>
            {plan ? (
              <span className="text-[11.5px] text-muted tnum">
                {plan.allocations.length} batch{plan.allocations.length === 1 ? "" : "es"} ·{" "}
                <span className="font-medium text-ink">{fmtMoney(plan.totalValue, true)}</span>
              </span>
            ) : null}
          </header>

          {preview.isLoading ? (
            <div className="flex items-center gap-2 px-3.5 py-3 text-[12px] text-muted">
              <Spinner size={13} /> Planning allocation…
            </div>
          ) : preview.isError ? (
            <p className="px-3.5 py-3 text-[12px] text-danger">
              Could not plan the allocation. {(preview.error as Error).message}
            </p>
          ) : shortfall > 0 ? (
            <div className="flex items-start gap-2.5 px-3.5 py-3">
              <AlertTriangle size={14} className="mt-0.5 shrink-0 text-danger" strokeWidth={1.75} />
              <div className="text-[12px] leading-snug">
                <span className="font-medium text-danger">
                  Only {fmtNum(plan?.allocations.reduce((a, b) => a + b.quantity, 0) ?? 0)} available.
                </span>{" "}
                <span className="text-muted">
                  Selling {fmtNum(quantity)} would leave {fmtNum(shortfall)} unit(s) short. Raise the
                  quantity or receive more stock first.
                </span>
              </div>
            </div>
          ) : (plan?.allocations.length ?? 0) === 0 ? (
            <p className="px-3.5 py-3 text-[12px] text-muted">
              No sellable batches.{allowExpired ? "" : " Expired batches are excluded."}
            </p>
          ) : (
            <ul className="divide-y divide-border/60">
              {plan?.allocations.map((a) => {
                const tone = expiryTone(a.expiryDate);
                return (
                  <li key={a.batchId} className="flex items-center gap-3 px-3.5 py-2 text-[12px]">
                    <span className="font-mono text-[11.5px] text-ink">{a.batchNo}</span>
                    <span className={`tnum text-[11px] ${tone.tone === "danger" ? "text-danger" : tone.tone === "warn" ? "text-warn" : "text-muted"}`}>
                      {a.expiryDate ? `${fmtDate(a.expiryDate)} · ${tone.label}` : "No expiry"}
                    </span>
                    <div className="flex-1" />
                    <span className="tnum text-muted">{fmtMoney(a.unitPrice, true)}</span>
                    <span className="tnum w-[52px] text-right font-medium text-ink">
                      ×{fmtNum(a.quantity)}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}

          {hasExpired && !allowExpired ? (
            <p className="border-t border-border px-3.5 py-2 text-[11.5px] text-muted">
              Expired stock is excluded by default. Tick the override below to include it.
            </p>
          ) : null}
        </div>

        <label className="flex cursor-pointer items-center gap-2.5 text-[12.5px] text-ink-2">
          <input
            type="checkbox"
            checked={allowExpired}
            onChange={(e) => setAllowExpired(e.target.checked)}
            className="h-3.5 w-3.5 accent-[var(--color-accent)]"
          />
          Allow drawing from expired batches
        </label>
      </div>
    </Modal>
  );
}