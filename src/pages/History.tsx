import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowDownLeft, ArrowUpRight, Download, Filter, History as HistoryIcon, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { api, saveTextFile } from "../lib/api";
import { useApp } from "../lib/ctx";
import { fmtDate, fmtMoney, fmtNum, fmtTime, KIND_META } from "../lib/format";
import type { HistoryDay, HistoryFilter, LedgerEntry } from "../lib/types";
import { EmptyState, ErrorState, SegTab, SkeletonRows } from "../components/ui";

const KIND_TABS = [
  { value: "", label: "All" },
  { value: "restock", label: "Restocks" },
  { value: "sale", label: "Sales" },
  { value: "adjustment", label: "Adjustments" },
];

const PAGE = 120;
const LEDGER_COLUMNS = [
  { label: "Time", width: 92 },
  { label: "Type", width: 112 },
  { label: "Product" },
  { label: "Batch", width: 128 },
  { label: "Buyer", width: 172 },
  { label: "Change", width: 100, num: true },
  { label: "Unit price", width: 112, num: true },
  { label: "Line total", width: 124, num: true },
];

export function History() {
  const { toast, reportError } = useApp();
  const [kinds, setKinds] = useState<string[]>([]);
  const [buyer, setBuyer] = useState("");
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [limit, setLimit] = useState(PAGE);

  useEffect(() => {
    const t = window.setTimeout(() => setDebounced(search), 200);
    return () => window.clearTimeout(t);
  }, [search]);

  const filter: HistoryFilter = {
    kinds: kinds.length ? kinds : null,
    buyerName: buyer || null,
    search: debounced.trim() || null,
    from: from || null,
    to: to || null,
    limit,
  };

  const q = useQuery({ queryKey: ["history", filter], queryFn: () => api.history(filter) });
  const buyers = q.data?.buyers ?? [];
  const activeFilters =
    kinds.length + (buyer ? 1 : 0) + (debounced.trim() ? 1 : 0) + (from ? 1 : 0) + (to ? 1 : 0);

  const exportCsv = useMutation({
    mutationFn: () => api.exportCsv(filter),
    onSuccess: async (res) => {
      const path = await saveTextFile(res.filename, res.content);
      if (path) toast("ok", `Exported ${fmtNum(res.rows)} entries`, path.split("/").pop());
    },
    onError: (e) => reportError(e, "Export failed"),
  });

  const clearAll = () => {
    setKinds([]);
    setBuyer("");
    setSearch("");
    setFrom("");
    setTo("");
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* ── Toolbar: floating chrome ─────────────────────────────────────── */}
      <div className="toolbar-glass shrink-0 border-b border-[var(--hairline)] px-5 py-2.5">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1" role="group" aria-label="Filter by movement type">
            {KIND_TABS.map((t) => {
              const on = t.value === "" ? kinds.length === 0 : kinds.includes(t.value);
              return (
                <SegTab
                  key={t.value || "all"}
                  active={on}
                  onClick={() =>
                    t.value === ""
                      ? setKinds([])
                      : setKinds((k) =>
                          k.includes(t.value) ? k.filter((x) => x !== t.value) : [...k, t.value],
                        )
                  }
                >
                  {t.label}
                </SegTab>
              );
            })}
          </div>

          <select
            value={buyer}
            onChange={(e) => setBuyer(e.target.value)}
            className="input w-[168px]"
            aria-label="Filter by buyer"
          >
            <option value="">All buyers</option>
            {buyers.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>

          <input
            type="date"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
            className="input tnum w-[138px]"
            aria-label="From date"
          />
          <span className="text-[11.5px] text-faint">to</span>
          <input
            type="date"
            value={to}
            onChange={(e) => setTo(e.target.value)}
            className="input tnum w-[138px]"
            aria-label="To date"
          />

          <div className="relative min-w-[180px] flex-1">
            <Search
              size={14}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint"
              strokeWidth={1.9}
            />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Product, batch, note…"
              className="input !pl-8"
              aria-label="Search history"
            />
          </div>

          {activeFilters > 0 ? (
            <button onClick={clearAll} className="btn btn-quiet !px-2.5">
              <Filter size={13.5} strokeWidth={1.9} />
              Clear {activeFilters}
            </button>
          ) : null}

          <button
            onClick={() => exportCsv.mutate()}
            disabled={exportCsv.isPending || (q.data?.totalRows ?? 0) === 0}
            className="btn btn-ghost"
          >
            <Download size={14} strokeWidth={1.9} />
            CSV
          </button>
        </div>

        {q.data ? (
          <div className="mt-2.5 flex flex-wrap items-center gap-x-5 gap-y-1 text-[11.5px]">
            <span className="flex items-center gap-1.5 text-muted">
              <HistoryIcon size={12} strokeWidth={1.9} />
              <span className="tnum font-[510] text-ink">{fmtNum(q.data.totalRows)}</span>
              entries
            </span>
            <span className="flex items-center gap-1.5 text-ok-ink">
              <ArrowDownLeft size={12} strokeWidth={1.9} />
              in <span className="tnum font-[510]">{fmtNum(q.data.unitsIn)}</span> units ·{" "}
              <span className="tnum font-[510]">{fmtMoney(q.data.valueIn)}</span>
            </span>
            <span className="flex items-center gap-1.5 text-accent">
              <ArrowUpRight size={12} strokeWidth={1.9} />
              out <span className="tnum font-[510]">{fmtNum(q.data.unitsOut)}</span> units ·{" "}
              <span className="tnum font-[510]">{fmtMoney(q.data.valueOut)}</span>
            </span>
          </div>
        ) : null}
      </div>

      {/* ── Timeline ───────────────────────────────────────────────────────── */}
      <div className="scroll-y min-h-0 flex-1">
        {q.isError ? (
          <ErrorState message={(q.error as Error).message} onRetry={() => void q.refetch()} />
        ) : q.isLoading ? (
          <table className="tbl">
            <colgroup>
              {LEDGER_COLUMNS.map((c, i) => (
                <col key={i} style={c.width ? { width: c.width } : undefined} />
              ))}
            </colgroup>
            <thead>
              <tr>
                {LEDGER_COLUMNS.map((c, i) => (
                  <th key={i} className={c.num ? "num" : ""}>
                    {c.label}
                  </th>
                ))}
              </tr>
            </thead>
            <SkeletonRows
              rows={14}
              cols={LEDGER_COLUMNS.length}
              align={LEDGER_COLUMNS.map((c) => (c.num ? "right" : "left"))}
            />
          </table>
        ) : (q.data?.days.length ?? 0) === 0 ? (
          <EmptyState
            icon={<HistoryIcon size={19} />}
            title="No movements yet"
            body={
              activeFilters > 0
                ? "No ledger entries match these filters. Widen the date range or clear the filters."
                : "Every restock, sale and adjustment will appear here as it happens."
            }
            action={
              activeFilters > 0 ? (
                <button onClick={clearAll} className="btn btn-ghost">
                  Clear filters
                </button>
              ) : undefined
            }
          />
        ) : (
          /* A single <table>, not one table per day. Each group auto-sized independently, so
             the batch column sat under the product column on one day and further right on the
             next — and because no group carried a <thead>, there were no column labels at all.
             Day separators are now full-width rows spanning every column. */
          <table className="tbl">
            <colgroup>
              <col style={{ width: 92 }} />
              <col style={{ width: 112 }} />
              <col />
              <col style={{ width: 128 }} />
              <col style={{ width: 172 }} />
              <col style={{ width: 100 }} />
              <col style={{ width: 112 }} />
              <col style={{ width: 124 }} />
            </colgroup>
            <thead>
              <tr>
                <th>Time</th>
                <th>Type</th>
                <th>Product</th>
                <th>Batch</th>
                <th>Buyer</th>
                <th className="num">Change</th>
                <th className="num">Unit price</th>
                <th className="num">Line total</th>
              </tr>
            </thead>
            <tbody>
              {q.data?.days.map((day) => (
                <DayGroup key={day.date} day={day} />
              ))}
            </tbody>
          </table>
        )}

        {q.data && q.data.totalRows > limit ? (
          <div className="flex items-center justify-center gap-3 border-t border-border px-5 py-4">
            <span className="text-[11.5px] text-muted tnum">
              Showing {fmtNum(limit)} of {fmtNum(q.data.totalRows)}
            </span>
            <button onClick={() => setLimit((l) => l + PAGE)} className="btn btn-ghost">
              Load more
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** One day: a full-width separator row carrying the date and subtotals, then its entries. */
function DayGroup({ day }: { day: HistoryDay }) {
  return (
    <>
      <tr>
        <td colSpan={8} className="!h-0 !border-b-0 !p-0">
          <div className="sticky top-[30px] z-[1] flex items-center gap-3 border-y border-[var(--hairline)] bg-canvas/95 px-5 py-2 backdrop-blur">
            <h3 className="text-[12px] font-[590] tracking-tight text-ink">{fmtDate(day.date)}</h3>
            <div className="flex-1" />
            {day.unitsIn > 0 ? (
              <span className="tnum text-[11.5px] text-ok-ink">+{fmtNum(day.unitsIn)} in</span>
            ) : null}
            {day.unitsOut > 0 ? (
              <span className="tnum text-[11.5px] text-accent">−{fmtNum(day.unitsOut)} out</span>
            ) : null}
            <span className="tnum text-[11.5px] text-muted">
              net {fmtMoney(day.valueOut - day.valueIn, true)}
            </span>
          </div>
        </td>
      </tr>
      {day.entries.map((e) => (
        <EntryRow key={e.id} e={e} />
      ))}
    </>
  );
}

function EntryRow({ e }: { e: LedgerEntry }) {
  const meta = KIND_META[e.kind] ?? { label: e.kind, tone: "pill-neutral" };
  const inbound = e.qtyDelta > 0;
  return (
    <tr>
      <td className="tnum whitespace-nowrap text-[12px] text-muted">{fmtTime(e.ts)}</td>
      <td>
        <span className={`pill ${meta.tone}`}>{meta.label}</span>
      </td>
      <td className="max-w-0">
        <div className="flex items-center gap-2">
          <span className="min-w-0 truncate font-[510] text-ink">{e.productName}</span>
          {e.note ? (
            <span className="min-w-0 truncate text-[11px] text-faint">{e.note}</span>
          ) : null}
          {e.source === "agent" ? (
            <span className="pill pill-accent shrink-0 !h-[17px] !px-1.5 !text-[10px]">
              agent{e.actor ? `: ${e.actor.replace("agent:", "")}` : ""}
            </span>
          ) : null}
        </div>
      </td>
      <td className="font-mono text-[11.5px]">
        <span className="block truncate">{e.batchNo ?? "—"}</span>
      </td>
      <td>
        <span className={`block truncate text-[12px] ${e.buyerName ? "text-ink-2" : "text-faint"}`}>
          {e.buyerName ?? "—"}
        </span>
      </td>
      <td
        className={`num tnum font-[510] ${
          inbound ? "text-ok-ink" : e.qtyDelta < 0 ? "text-accent" : "text-muted"
        }`}
      >
        {inbound ? "+" : ""}
        {fmtNum(e.qtyDelta)}
      </td>
      <td className="num tnum">{fmtMoney(e.unitPrice, true)}</td>
      <td className="num tnum font-[510] text-ink">{fmtMoney(Math.abs(e.totalValue), true)}</td>
    </tr>
  );
}