import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Cpu, Info, Lightbulb } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { fmtMoney, fmtNum, SEVERITY_META } from "../lib/format";
import type { Insight } from "../lib/types";
import { EmptyState, ErrorState, Spinner } from "../components/ui";

/**
 * Deterministic analysis. Every card states the rule that produced it — a heuristic nobody can
 * audit is a heuristic nobody trusts. No model runs on this page, which is also why it is
 * instant and costs nothing in RAM.
 */
export function Insights() {
  const q = useQuery({
    queryKey: ["insights"],
    queryFn: api.insights,
    staleTime: 30_000,
  });

  if (q.isError) {
    return <ErrorState message={(q.error as Error).message} onRetry={() => void q.refetch()} />;
  }
  if (q.isLoading) {
    return (
      <div className="flex items-center gap-2.5 p-8 text-[13px] text-muted">
        <Spinner /> Running analyses…
      </div>
    );
  }

  const d = q.data;
  if (!d || d.insights.length === 0) {
    return (
      <EmptyState
        icon={<Lightbulb size={19} />}
        title="No analyses to show"
        body="Insights need stock and movement data. Receive some stock and record a few sales."
      />
    );
  }

  const severityRank = { critical: 0, warning: 1, info: 2 } as const;
  const ordered = [...d.insights].sort(
    (a, b) => severityRank[a.severity] - severityRank[b.severity] || b.itemCount - a.itemCount,
  );

  return (
    <div className="space-y-3 p-5">
      <div className="flex items-center gap-2.5 text-[11.5px] text-muted">
        <Cpu size={13} strokeWidth={1.75} />
        <span>
          {d.insights.length} analyses ·{" "}
          <span className="tnum font-medium text-ink">{fmtNum(d.totalFlagged)}</span> items flagged
        </span>
        <span className="text-faint">· computed by SQL, no model involved</span>
      </div>

      {ordered.map((ins) => (
        <InsightCard key={ins.id} insight={ins} />
      ))}
    </div>
  );
}

function InsightCard({ insight }: { insight: Insight }) {
  const [open, setOpen] = useState(false);
  const sev = SEVERITY_META[insight.severity] ?? SEVERITY_META.info;
  const empty = insight.itemCount === 0;

  return (
    <section className={`card overflow-hidden ${empty ? "opacity-70" : ""}`}>
      <header className="flex items-start gap-3 px-4 py-3.5">
        <span className={`pill ${sev.tone} mt-0.5 shrink-0`}>{sev.label}</span>

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2.5">
            <h2 className="text-[13.5px] font-semibold tracking-tight text-ink">
              {insight.title}
            </h2>
            <span className="tnum text-[11.5px] text-muted">
              {fmtNum(insight.itemCount)} item{insight.itemCount === 1 ? "" : "s"}
            </span>
            {insight.metric !== null && insight.metricLabel ? (
              <span className="tnum text-[11.5px] text-ink-2">
                · {insight.metricLabel}: <span className="font-medium">{fmtMoney(insight.metric, true)}</span>
              </span>
            ) : null}
          </div>
          <p className="mt-1 text-[12px] leading-snug text-ink-2">{insight.detail}</p>
          <p className="mt-1.5 flex items-start gap-1.5 text-[11px] leading-snug text-faint">
            <Info size={11} className="mt-0.5 shrink-0" strokeWidth={1.75} />
            <span>
              <span className="font-medium">Rule:</span> {insight.rule}
            </span>
          </p>
        </div>

        {insight.items.length > 0 ? (
          <button
            onClick={() => setOpen((o) => !o)}
            className="btn btn-quiet shrink-0 !px-2"
            aria-expanded={open}
          >
            {open ? "Hide" : "Show"}
            <ChevronDown size={13} className={open ? "rotate-180" : ""} />
          </button>
        ) : null}
      </header>

      {open && insight.items.length > 0 ? (
        <div className="border-t border-border">
          <table className="tbl">
            <thead>
              <tr>
                <th>Item</th>
                {insight.items.some((i) => i.batchNo) ? <th>Batch</th> : null}
                {insight.items.some((i) => i.quantity !== null) ? (
                  <th className="num">Quantity</th>
                ) : null}
                {insight.items.some((i) => i.value !== null) ? (
                  <th className="num">Value</th>
                ) : null}
                {insight.items.some((i) => i.days !== null) ? <th className="num">Days</th> : null}
                <th>Suggested action</th>
              </tr>
            </thead>
            <tbody>
              {insight.items.map((it, i) => (
                <tr key={`${it.productName}-${i}`}>
                  <td className="font-medium text-ink">{it.productName}</td>
                  {insight.items.some((x) => x.batchNo) ? (
                    <td className="font-mono text-[11.5px]">{it.batchNo ?? "—"}</td>
                  ) : null}
                  {insight.items.some((x) => x.quantity !== null) ? (
                    <td className="num tnum">{fmtNum(it.quantity)}</td>
                  ) : null}
                  {insight.items.some((x) => x.value !== null) ? (
                    <td className="num tnum">{it.value === null ? "—" : fmtMoney(it.value, true)}</td>
                  ) : null}
                  {insight.items.some((x) => x.days !== null) ? (
                    <td className="num tnum">{it.days ?? "—"}</td>
                  ) : null}
                  <td className="text-[12px] text-muted">{it.suggestion ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}