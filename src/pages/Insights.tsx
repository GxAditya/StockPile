import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Cpu, Info, Lightbulb } from "lucide-react";
import { useState, type CSSProperties } from "react";
import { api } from "../lib/api";
import { fmtMoney, fmtNum, SEVERITY_META } from "../lib/format";
import type { Insight } from "../lib/types";
import { Disclosure, EmptyState, ErrorState, Skeleton, SkeletonText } from "../components/ui";

/**
 * Deterministic analysis. Every card states the rule that produced it — a
 * heuristic nobody can audit is a heuristic nobody trusts. No model runs on
 * this page, which is also why it is instant and costs nothing in RAM.
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
  if (q.isLoading) return <InsightsSkeleton />;

  const d = q.data;
  if (!d || d.insights.length === 0) {
    return (
      <EmptyState
        icon={<Lightbulb size={19} strokeWidth={1.9} />}
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
        <Cpu size={13} strokeWidth={1.9} />
        <span>
          {d.insights.length} analyses ·{" "}
          <span className="tnum font-[510] text-ink">{fmtNum(d.totalFlagged)}</span> items flagged
        </span>
        <span className="text-faint">· computed by SQL, no model involved</span>
      </div>

      {ordered.map((ins, i) => (
        <InsightCard key={ins.id} insight={ins} index={i} />
      ))}
    </div>
  );
}

function InsightCard({ insight, index }: { insight: Insight; index: number }) {
  const [open, setOpen] = useState(false);
  const sev = SEVERITY_META[insight.severity] ?? SEVERITY_META.info;
  const glyph = insight.severity === "critical" ? "✕" : insight.severity === "warning" ? "◐" : "●";
  const empty = insight.itemCount === 0;

  return (
    <section
      className={`card rise-in overflow-hidden ${empty ? "opacity-70" : ""}`}
      // Staggered entrance, but capped so a long list never crawls in.
      style={{ "--i": Math.min(index, 8) } as CSSProperties}
    >
      <header className="flex items-start gap-3 px-4 py-3">
        <span className={`pill ${sev.tone} mt-0.5 shrink-0`}>
          <span aria-hidden>{glyph}</span>
          {sev.label}
        </span>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2.5">
            <h2 className="title-tight text-[13px] font-[590] text-ink">{insight.title}</h2>
            <span className="tnum text-[11.5px] text-muted">
              {fmtNum(insight.itemCount)} item{insight.itemCount === 1 ? "" : "s"}
            </span>
            {insight.metric !== null && insight.metricLabel ? (
              <span className="tnum text-[11.5px] text-ink-2">
                · {insight.metricLabel}:{" "}
                <span className="font-[510]">{fmtMoney(insight.metric, true)}</span>
              </span>
            ) : null}
          </div>
          <p className="mt-1 text-[12px] leading-snug text-ink-2">{insight.detail}</p>
          <p className="mt-1.5 flex items-start gap-1.5 text-[11px] leading-snug text-faint">
            <Info size={11} className="mt-0.5 shrink-0" strokeWidth={1.9} />
            <span>
              <span className="font-[510]">Rule:</span> {insight.rule}
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
            <ChevronDown
              size={13}
              aria-hidden
              className={`text-muted transition-transform duration-[280ms] ease-[cubic-bezier(0.32,0.72,0.35,1)] ${
                open ? "rotate-180" : "rotate-0"
              }`}
            />
          </button>
        ) : null}
      </header>

      {insight.items.length > 0 ? (
        <Disclosure open={open}>
          <div className="border-t border-[var(--hairline)]">
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
                    <td className="font-[510] text-ink">{it.productName}</td>
                    {insight.items.some((x) => x.batchNo) ? (
                      <td className="font-mono text-[11.5px]">{it.batchNo ?? "—"}</td>
                    ) : null}
                    {insight.items.some((x) => x.quantity !== null) ? (
                      <td className="num tnum">{fmtNum(it.quantity)}</td>
                    ) : null}
                    {insight.items.some((x) => x.value !== null) ? (
                      <td className="num tnum">
                        {it.value === null ? "—" : fmtMoney(it.value, true)}
                      </td>
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
        </Disclosure>
      ) : null}
    </section>
  );
}

/**
 * Mirrors the card stack — same chrome, paddings and rhythm — so results land
 * in place rather than replacing a spinner and moving everything.
 */
function InsightsSkeleton() {
  return (
    <div className="space-y-3 p-5" aria-busy="true" aria-label="Running analyses">
      <div className="flex items-center gap-2.5">
        <Skeleton h={13} w={13} rounded={4} />
        <Skeleton h={9} w={228} />
      </div>
      {Array.from({ length: 4 }).map((_, i) => (
        <section key={i} className="card px-4 py-3">
          <div className="flex items-start gap-3">
            <Skeleton h={20} w={72} rounded={999} />
            <div className="min-w-0 flex-1">
              <Skeleton h={11} w="38%" />
              <SkeletonText lines={2} className="mt-2" />
              <Skeleton h={8} w="54%" className="mt-2" />
            </div>
            <Skeleton h={26} w={64} rounded={7} />
          </div>
        </section>
      ))}
    </div>
  );
}
