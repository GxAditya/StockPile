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
import {
  Bar,
  BarChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../lib/api";
import { fmtDateShort, fmtMoney, fmtNum, expiryTone } from "../lib/format";
import { ErrorState } from "../components/ui";

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
  if (!d) {
    return (
      <div className="space-y-4 p-5">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <div key={i} className="card card-pad">
              <div className="skeleton h-2.5 w-24" />
              <div className="skeleton mt-3 h-6 w-16" />
            </div>
          ))}
        </div>
        <div className="card h-[240px] p-5">
          <div className="skeleton h-full w-full" />
        </div>
      </div>
    );
  }

  const k = d.kpis;
  const expiredValue = d.expiryHorizon[0]?.value ?? 0;
  const expiringValue =
    (d.expiryHorizon[1]?.value ?? 0) + (d.expiryHorizon[2]?.value ?? 0);
  const expiryWarningCount = k.expiredBatches + k.expiring30dBatches;
  const lowStockCount = k.lowStockCount + k.outOfStockCount;

  return (
    <div className="space-y-4 p-5">
      {/* ── The three things that need a decision ────────────────────────────────
          Three tiles, not eight. An operator opening this page needs to know what is
          going wrong and how big the catalogue is. Stock value, unit counts, revenue and
          buyer counts live on the pages that own them. */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <Kpi
          label="Expiry warning"
          value={fmtNum(expiryWarningCount)}
          icon={AlertTriangle}
          tone={expiryWarningCount > 0 ? "warn" : undefined}
          hint={`${k.expiredBatches} expired · ${k.expiring30dBatches} within 30d`}
        />
        <Kpi
          label="Low stock warning"
          value={fmtNum(lowStockCount)}
          icon={PackageX}
          tone={lowStockCount > 0 ? "warn" : undefined}
          hint={`${k.outOfStockCount} out of stock`}
        />
        <Kpi
          label="Products in inventory"
          value={fmtNum(k.productCount)}
          icon={Layers}
          hint={`${fmtNum(k.totalUnits)} units on hand`}
        />
      </div>

      {/* ── Needs attention ────────────────────────────────────────────────── */}
      {d.needsAttention.length > 0 ? (
        <section className="card overflow-hidden">
          <header className="flex items-center gap-2.5 border-b border-border px-4 py-3">
            <CircleAlert size={15} className="text-warn" strokeWidth={1.75} />
            <h2 className="text-[13px] font-semibold text-ink">Needs attention</h2>
            <span className="pill pill-warn tnum">{d.needsAttention.length}</span>
            <div className="flex-1" />
            <span className="text-[11.5px] text-muted tnum">
              {fmtMoney(expiredValue)} expired · {fmtMoney(expiringValue)} expiring ≤30d
            </span>
          </header>
          <div className="scroll-y max-h-[236px]">
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
                          className="font-medium text-ink hover:text-cyan"
                        >
                          {a.productName}
                        </Link>
                      </td>
                      <td className="font-mono text-[12px]">{a.batchNo}</td>
                      <td className="tnum">{fmtDateShort(a.expiryDate)}</td>
                      <td>
                        <span
                          className={`pill ${
                            tone.tone === "danger" ? "pill-danger" : tone.tone === "warn" ? "pill-warn" : "pill-info"
                          }`}
                        >
                          {tone.tone === "danger" ? "✕" : "◐"} {tone.label}
                        </span>
                      </td>
                      <td className="num tnum">{fmtNum(a.quantity)}</td>
                      <td className="num tnum">{fmtMoney(a.unitPrice, true)}</td>
                      <td className="num tnum font-medium text-danger">
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
        <section className="card card-pad">
          <ChartHead title="Top sellers" sub="Units sold, last 30 days" />
          <div className="h-[196px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={d.topSold30d}
                layout="vertical"
                margin={{ top: 4, right: 30, bottom: 0, left: 4 }}
              >
                <XAxis type="number" stroke="#56657a" tick={{ fontSize: 10.5 }} tickLine={false} axisLine={false} />
                <YAxis
                  type="category"
                  dataKey="name"
                  width={122}
                  stroke="#8a9aaf"
                  tick={{ fontSize: 10.5 }}
                  tickLine={false}
                  axisLine={false}
                  tickFormatter={(v: string) => (v.length > 20 ? `${v.slice(0, 19)}…` : v)}
                />
                <Tooltip content={<ChartTip />} cursor={{ fill: "#151e2a" }} />
                <Bar dataKey="units" name="Units" fill="#6366f1" radius={[0, 4, 4, 0]} barSize={15} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </section>

        <section className="card card-pad">
          <ChartHead title="Expiry horizon" sub="Units by remaining shelf life" />
          <ul className="space-y-2.5 pt-1">
            {d.expiryHorizon.map((b, i) => {
              const max = Math.max(...d.expiryHorizon.map((x) => x.units), 1);
              const colours = ["#f43f5e", "#fb923c", "#f59e0b", "#38bdf8", "#10b981"];
              return (
                <li key={b.bucket}>
                  <div className="flex items-baseline justify-between text-[12px]">
                    <span className="text-ink-2">{b.bucket}</span>
                    <span className="flex items-baseline gap-2">
                      <span className="tnum text-muted">{b.batches} batch</span>
                      <span className="tnum w-[52px] text-right font-medium text-ink">
                        {fmtNum(b.units)}
                      </span>
                    </span>
                  </div>
                  <div className="mt-1 h-[5px] w-full overflow-hidden rounded-full bg-surface-3">
                    <div
                      className="h-full rounded-full"
                      style={{ width: `${(b.units / max) * 100}%`, background: colours[i] }}
                    />
                  </div>
                </li>
              );
            })}
          </ul>
        </section>

        <section className="card card-pad">
          <ChartHead title="This month" sub="Throughput against last 30 days" />
          <div className="space-y-3 pt-1">
            <Throughput
              icon={ArrowUpRight}
              label="Units received"
              value={fmtNum(k.unitsAdded30d)}
              tone="text-ok"
            />
            <Throughput
              icon={ArrowDownRight}
              label="Units sold"
              value={fmtNum(k.unitsSold30d)}
              tone="text-accent-soft"
            />
            <div className="h-px bg-border" />
            <Throughput
              icon={TrendingUp}
              label="Net movement"
              value={fmtNum(k.unitsAdded30d - k.unitsSold30d)}
              tone={k.unitsAdded30d >= k.unitsSold30d ? "text-ok" : "text-danger"}
            />
            <div className="h-px bg-border" />
            <div className="flex items-baseline justify-between text-[12px]">
              <span className="text-muted">Avg revenue / unit sold</span>
              <span className="tnum font-medium text-ink">
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

function Kpi({
  label,
  value,
  icon: Icon,
  tone,
  hint,
}: {
  label: string;
  value: string;
  icon: typeof Boxes;
  tone?: "warn" | "danger";
  hint?: string;
}) {
  return (
    <div className="card card-pad !p-3.5">
      <div className="flex items-start gap-1.5">
        <Icon size={12.5} className="mt-[2px] shrink-0 text-faint" strokeWidth={1.75} />
        <span className="kpi-label leading-[1.25]">{label}</span>
      </div>
      <div
        className={`tnum mt-2 text-[19px] font-semibold leading-none tracking-tight ${
          tone === "warn" ? "text-warn" : tone === "danger" ? "text-danger" : "text-ink"
        }`}
      >
        {value}
      </div>
      {hint ? <div className="mt-1.5 truncate text-[11px] text-faint tnum">{hint}</div> : null}
    </div>
  );
}

function ChartHead({ title, sub }: { title: string; sub: string }) {
  return (
    <header className="mb-3 flex items-start justify-between gap-3">
      <div>
        <h2 className="text-[13px] font-semibold tracking-tight text-ink">{title}</h2>
        <p className="mt-0.5 text-[11.5px] text-muted">{sub}</p>
      </div>
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
        <Icon size={14} strokeWidth={1.75} />
        {label}
      </span>
      <span className={`tnum font-medium ${tone}`}>{value}</span>
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
    <div className="card px-2.5 py-2 shadow-xl shadow-black/50">
      {label !== undefined ? (
        <div className="mb-1 text-[11px] text-muted">{fmtDateShort(String(label))}</div>
      ) : null}
      <div className="space-y-0.5">
        {rows.map((p, i) => (
          <div key={i} className="flex items-center gap-2 text-[12px]">
            <span
              className="h-[7px] w-[7px] shrink-0 rounded-[2px]"
              style={{ background: p.color ?? "#6366f1" }}
            />
            <span className="text-muted">{p.name ?? p.dataKey}</span>
            <span className="tnum ml-auto pl-3 font-medium text-ink">{fmtNum(p.value)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
