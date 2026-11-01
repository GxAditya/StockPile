import { useQuery } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowDownRight,
  ArrowUpRight,
  Boxes,
  CircleAlert,
  Layers,
  PackageX,
  TrendingUp,
} from "lucide-react";
import { Link } from "react-router-dom";
import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api } from "../lib/api";
import { fmtDateShort, fmtMoney, fmtNum, expiryTone } from "../lib/format";
import { prefersReducedMotion } from "../lib/motion";
import {
  AnimatedNumber,
  ErrorState,
  Skeleton,
  SkeletonRows,
  SkeletonText,
} from "../components/ui";

/** Expired → fresh. An ordered ramp, so position in the list reads as urgency. */
export const CHART_RAMP = [1, 2, 3, 4, 5, 6].map((i) => `var(--color-chart-${i})`);

export function Dashboard() {
  const q = useQuery({
    queryKey: ["dashboard"],
    queryFn: api.dashboard,
    refetchInterval: 30_000,
  });

  if (q.isError) {
    return <ErrorState message={(q.error as Error).message} onRetry={() => void q.refetch()} />;
  }

  const d = q.data;
  if (!d) return <DashboardSkeleton />;

  const k = d.kpis;
  const expiredValue = d.expiryHorizon[0]?.value ?? 0;
  const expiringValue = (d.expiryHorizon[1]?.value ?? 0) + (d.expiryHorizon[2]?.value ?? 0);
  const expiryWarningCount = k.expiredBatches + k.expiring30dBatches;
  const lowStockCount = k.lowStockCount + k.outOfStockCount;

  return (
    <div className="space-y-3 p-5">
      {/* Three tiles, not eight. An operator opening this page needs to know
          what is going wrong and how big the catalogue is; stock value, unit
          counts and revenue live on the pages that own them. */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <Kpi
          i={0}
          label="Expiry warning"
          value={expiryWarningCount}
          icon={AlertTriangle}
          tone={expiryWarningCount > 0 ? "warn" : undefined}
          hint={`${k.expiredBatches} expired · ${k.expiring30dBatches} within 30d`}
        />
        <Kpi
          i={1}
          label="Low stock warning"
          value={lowStockCount}
          icon={PackageX}
          tone={lowStockCount > 0 ? "warn" : undefined}
          hint={`${k.outOfStockCount} out of stock`}
        />
        <Kpi
          i={2}
          label="Products in inventory"
          value={k.productCount}
          icon={Layers}
          hint={`${fmtNum(k.totalUnits)} units on hand`}
        />
      </div>

      {d.needsAttention.length > 0 ? (
        <section className="card rise-in overflow-hidden" style={{ "--i": 3 } as React.CSSProperties}>
          <header className="flex flex-wrap items-center gap-2.5 border-b border-[var(--hairline)] px-4 py-2.5">
            <CircleAlert size={14.5} className="text-warn-ink" strokeWidth={1.9} />
            <h2 className="text-[12.5px] font-[590] text-ink">Needs attention</h2>
            <span className="pill pill-warn tnum">{d.needsAttention.length}</span>
            <div className="flex-1" />
            <span className="tnum text-[11.5px] text-muted">
              {fmtMoney(expiredValue)} expired · {fmtMoney(expiringValue)} expiring ≤30d
            </span>
          </header>
          <div className="scroll-y max-h-[240px]">
            <table className="tbl">
              <thead>
                <tr>
                  <th>Product</th>
                  <th>Batch</th>
                  <th>Expiry</th>
                  <th>Status</th>
                  <th className="num">Qty</th>
                  <th className="num">Unit price</th>
                  <th className="num">Value at risk</th>
                </tr>
              </thead>
              <tbody>
                {d.needsAttention.map((a) => {
                  const tone = expiryTone(a.expiryDate);
                  return (
                    <tr key={a.batchId}>
                      <td>
                        <Link
                          to={`/inventory?focus=${a.productId}`}
                          className="font-[510] text-ink hover:text-accent"
                        >
                          {a.productName}
                        </Link>
                      </td>
                      <td className="font-mono text-[11.5px]">{a.batchNo}</td>
                      <td className="tnum">{fmtDateShort(a.expiryDate)}</td>
                      <td>
                        <span
                          className={`pill ${
                            tone.tone === "danger"
                              ? "pill-danger"
                              : tone.tone === "warn"
                                ? "pill-warn"
                                : "pill-info"
                          }`}
                        >
                          <span aria-hidden>{tone.tone === "danger" ? "✕" : "◐"}</span>
                          {tone.label}
                        </span>
                      </td>
                      <td className="num tnum">{fmtNum(a.quantity)}</td>
                      <td className="num tnum">{fmtMoney(a.unitPrice, true)}</td>
                      <td className="num tnum font-[590] text-danger-ink">
                        {fmtMoney(a.valueAtRisk, true)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>
      ) : null}

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <section
          className="card card-pad rise-in"
          style={{ "--i": 4 } as React.CSSProperties}
          aria-label="Top sellers"
        >
          <ChartHead title="Top sellers" sub="Units sold, last 30 days" />
          <div className="h-[196px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={d.topSold30d}
                layout="vertical"
                margin={{ top: 4, right: 28, bottom: 0, left: 4 }}
              >
                <XAxis
                  type="number"
                  stroke="var(--color-faint)"
                  tick={{ fontSize: 10.5 }}
                  tickLine={false}
                  axisLine={false}
                />
                <YAxis
                  type="category"
                  dataKey="name"
                  width={118}
                  stroke="var(--color-muted)"
                  tick={{ fontSize: 10.5 }}
                  tickLine={false}
                  axisLine={false}
                  tickFormatter={(v: string) => (v.length > 19 ? `${v.slice(0, 18)}…` : v)}
                />
                <Tooltip
                  content={<ChartTip />}
                  cursor={{ fill: "var(--color-surface-2)" }}
                  animationDuration={120}
                />
                <Bar
                  dataKey="units"
                  name="Units"
                  fill="var(--color-accent)"
                  radius={[0, 4, 4, 0]}
                  barSize={14}
                  isAnimationActive={!prefersReducedMotion()}
                  animationDuration={480}
                  animationEasing="cubic-bezier(0.2,0,0,1)"
                />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </section>

        <section className="card card-pad rise-in" style={{ "--i": 5 } as React.CSSProperties}>
          <ChartHead title="Expiry horizon" sub="Units by remaining shelf life" />
          <ul className="space-y-2.5 pt-1">
            {d.expiryHorizon.map((b, i) => {
              const max = Math.max(...d.expiryHorizon.map((x) => x.units), 1);
              const pct = b.units / max;
              return (
                <li key={b.bucket}>
                  <div className="flex items-baseline justify-between text-[12px]">
                    <span className="text-ink-2">{b.bucket}</span>
                    <span className="flex items-baseline gap-2">
                      <span className="tnum text-muted">
                        {fmtNum(b.batches)} batch{b.batches === 1 ? "" : "es"}
                      </span>
                      <span className="tnum w-[52px] text-right font-[510] text-ink">
                        {fmtNum(b.units)}
                      </span>
                    </span>
                  </div>
                  {/* Scale on the compositor rather than animating width. */}
                  <div className="mt-1 h-[5px] w-full overflow-hidden rounded-full bg-surface-3">
                    <div
                      className="h-full w-full origin-left rounded-full"
                      style={{
                        background: CHART_RAMP[i] ?? "var(--color-accent)",
                        transform: `scaleX(${pct})`,
                        transition: `transform 520ms cubic-bezier(0.2,0,0,1) ${i * 45}ms`,
                      }}
                    />
                  </div>
                </li>
              );
            })}
          </ul>
        </section>

        <section className="card card-pad rise-in" style={{ "--i": 6 } as React.CSSProperties}>
          <ChartHead title="This month" sub="Throughput against last 30 days" />
          <div className="space-y-3 pt-1">
            <Throughput
              icon={ArrowUpRight}
              label="Units received"
              value={fmtNum(k.unitsAdded30d)}
              tone="text-ok-ink"
            />
            <Throughput
              icon={ArrowDownRight}
              label="Units sold"
              value={fmtNum(k.unitsSold30d)}
              tone="text-accent"
            />
            <div className="h-px bg-[var(--hairline)]" />
            <Throughput
              icon={TrendingUp}
              label="Net movement"
              value={fmtNum(k.unitsAdded30d - k.unitsSold30d)}
              tone={k.unitsAdded30d >= k.unitsSold30d ? "text-ok-ink" : "text-danger-ink"}
            />
            <div className="h-px bg-[var(--hairline)]" />
            <div className="flex items-baseline justify-between text-[12px]">
              <span className="text-muted">Avg revenue / unit sold</span>
              <span className="tnum font-[510] text-ink">
                {k.unitsSold30d > 0 ? fmtMoney(k.revenue30d / k.unitsSold30d, true) : "—"}
              </span>
            </div>
          </div>
          <Link to="/insights" className="btn btn-ghost mt-4 w-full">
            Open insights
          </Link>
        </section>
      </div>
    </div>
  );
}

/* ── Loading ─────────────────────────────────────────────────────────────────
   Mirrors the real layout: three tiles at the real height, a table at the real
   row height with the real column count, then the three chart cards. Arriving
   data therefore replaces this in place instead of shoving it down the page. */
function DashboardSkeleton() {
  const ATTENTION_COLUMNS = [
    { label: "Product" },
    { label: "Batch" },
    { label: "Expiry" },
    { label: "Status" },
    { label: "Qty", num: true },
    { label: "Unit price", num: true },
    { label: "Value at risk", num: true },
  ];

  return (
    <div className="space-y-3 p-5" aria-busy="true" aria-label="Loading dashboard">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="card !p-3.5">
            <div className="flex items-center gap-1.5">
              <Skeleton h={12} w={12} rounded={4} />
              <Skeleton h={9} w={104} />
            </div>
            <Skeleton h={19} w={54} className="mt-2.5" rounded={6} />
            <Skeleton h={9} w={132} className="mt-2" />
          </div>
        ))}
      </div>

      <section className="card overflow-hidden">
        <header className="flex items-center gap-2.5 border-b border-[var(--hairline)] px-4 py-2.5">
          <Skeleton h={14} w={14} rounded={5} />
          <Skeleton h={10} w={104} />
        </header>
        <table className="tbl">
          <thead>
            <tr>
              {ATTENTION_COLUMNS.map((c) => (
                <th key={c.label} className={c.num ? "num" : ""}>
                  {c.label}
                </th>
              ))}
            </tr>
          </thead>
          <SkeletonRows
            rows={5}
            cols={ATTENTION_COLUMNS.length}
            align={ATTENTION_COLUMNS.map((c) => (c.num ? "right" : "left"))}
          />
        </table>
      </section>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <div className="card card-pad">
          <Skeleton h={10} w={92} />
          <Skeleton h={9} w={148} className="mt-2" />
          <Skeleton h={196} rounded={8} className="mt-3.5" />
        </div>
        <div className="card card-pad">
          <Skeleton h={10} w={104} />
          <Skeleton h={9} w={160} className="mt-2" />
          <div className="mt-4 space-y-3.5">
            {Array.from({ length: 5 }).map((_, i) => (
              <div key={i}>
                <div className="flex justify-between">
                  <Skeleton h={9} w={68} />
                  <Skeleton h={9} w={40} />
                </div>
                <Skeleton h={5} rounded={999} className="mt-1.5" />
              </div>
            ))}
          </div>
        </div>
        <div className="card card-pad">
          <Skeleton h={10} w={84} />
          <Skeleton h={9} w={172} className="mt-2" />
          <SkeletonText lines={5} className="mt-4" />
        </div>
      </div>
    </div>
  );
}

function Kpi({
  i,
  label,
  value,
  icon: Icon,
  tone,
  hint,
}: {
  i: number;
  label: string;
  value: number;
  icon: typeof Boxes;
  tone?: "warn" | "danger";
  hint?: string;
}) {
  return (
    <div className="card rise-in !p-3.5" style={{ "--i": i } as React.CSSProperties}>
      <div className="flex items-start gap-1.5">
        <Icon
          size={12.5}
          className={`mt-[2px] shrink-0 ${tone === "warn" ? "text-warn-ink" : "text-faint"}`}
          strokeWidth={1.9}
        />
        <span className="kpi-label leading-[1.25]">{label}</span>
      </div>
      {/* Eased rather than snapped: a 30s background refetch would otherwise
          flicker figures with no sense of which way they moved. */}
      <AnimatedNumber
        value={value}
        format={fmtNum}
        className={`tnum mt-2 block text-[19px] leading-none font-[620] tracking-[-0.02em] ${
          tone === "warn"
            ? "text-warn-ink"
            : tone === "danger"
              ? "text-danger-ink"
              : "text-ink"
        }`}
      />
      {hint ? <div className="tnum mt-1.5 truncate text-[11px] text-faint">{hint}</div> : null}
    </div>
  );
}

function ChartHead({ title, sub }: { title: string; sub: string }) {
  return (
    <header className="mb-3">
      <h2 className="title-tight text-[12.5px] font-[590] text-ink">{title}</h2>
      <p className="mt-0.5 text-[11.5px] text-muted">{sub}</p>
    </header>
  );
}

function Throughput({
  icon: Icon,
  label,
  value,
  tone,
}: {
  icon: typeof Boxes;
  label: string;
  value: string;
  tone: string;
}) {
  return (
    <div className="flex items-center justify-between text-[12.5px]">
      <span className="flex items-center gap-2 text-muted">
        <Icon size={13.5} strokeWidth={1.9} />
        {label}
      </span>
      <span className={`tnum font-[510] ${tone}`}>{value}</span>
    </div>
  );
}

/** Shared tooltip so every chart reads the same way. */
function ChartTip({
  active,
  payload,
  label,
}: {
  active?: boolean;
  payload?: { name?: string; value?: number; color?: string; dataKey?: string | number }[];
  label?: string | number;
}) {
  if (!active || !payload?.length) return null;
  const rows = payload.filter((p) => p.value !== undefined && p.value !== null);
  return (
    <div className="glass-pop rounded-[10px] px-2.5 py-2">
      {label !== undefined ? (
        <div className="mb-1 text-[11px] text-muted">{fmtDateShort(String(label))}</div>
      ) : null}
      <div className="space-y-0.5">
        {rows.map((p, i) => (
          <div key={i} className="flex items-center gap-2 text-[12px]">
            <span
              className="h-[7px] w-[7px] shrink-0 rounded-[2px]"
              style={{ background: p.color ?? "var(--color-accent)" }}
            />
            <span className="text-muted">{p.name ?? p.dataKey}</span>
            <span className="tnum ml-auto pl-3 font-[510] text-ink">{fmtNum(p.value)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
