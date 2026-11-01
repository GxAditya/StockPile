import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Cpu, Database, HardDrive, Info, Monitor, RefreshCw, Trash2, Zap } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { useApp } from "../lib/ctx";
import { fmtNum } from "../lib/format";
import { haptic } from "../lib/motion";
import { APPEARANCE_OPTIONS, applyAppearance, type Appearance } from "../lib/theme";
import { Segmented, Skeleton, SkeletonRows, Spinner } from "../components/ui";

export function Settings({
  appearance,
  onAppearanceChange,
}: {
  appearance: Appearance;
  onAppearanceChange: (m: Appearance) => void;
}) {
  const { toast, reportError } = useApp();
  const qc = useQueryClient();

  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const health = useQuery({
    queryKey: ["agent-health"],
    queryFn: api.agentHealth,
    refetchInterval: 8000,
  });
  const insights = useQuery({ queryKey: ["insights"], queryFn: api.insights });
  const [confirmReset, setConfirmReset] = useState(false);

  const unload = useMutation({
    mutationFn: api.agentUnloadModels,
    onSuccess: () => {
      haptic("commit");
      toast("ok", "Models unloaded", "No model is held resident in RAM");
      void qc.invalidateQueries({ queryKey: ["agent-health"] });
    },
    onError: (e) => reportError(e, "Could not unload models"),
  });

  const reseed = useMutation({
    mutationFn: () => api.resetDatabase().then(() => api.seedDemo()),
    onSuccess: (r) => {
      haptic("commit");
      toast("ok", "Demo data rebuilt", `${r.products} products · ${r.batches} batches · ${r.sales} sales`);
      setConfirmReset(false);
      void qc.invalidateQueries();
    },
    onError: (e) => {
      haptic("error");
      reportError(e, "Could not rebuild demo data");
      setConfirmReset(false);
    },
  });

  const setOverride = useMutation({
    mutationFn: (v: { task: string; model: string | null }) => api.agentSetOverride(v.task, v.model),
    onSuccess: () => {
      haptic("tick");
      toast("ok", "Routing updated");
      void qc.invalidateQueries({ queryKey: ["agent-health"] });
    },
    onError: (e) => reportError(e, "Could not update routing"),
  });

  const h = health.data;
  const overrides = new Map((h?.overrides ?? []).map(([t, m]) => [t, m]));

  return (
    <div className="max-w-[840px] space-y-3 p-5">
      {/* ── Appearance ────────────────────────────────────────────────────── */}
      <section className="card card-pad rise-in" style={{ "--i": 0 } as React.CSSProperties}>
        <div className="flex flex-wrap items-center gap-2">
          <Monitor size={15} className="text-muted" strokeWidth={1.9} />
          <h2 className="title-tight text-[13px] font-[590] text-ink">Appearance</h2>
          <div className="flex-1" />
          <Segmented
            value={appearance}
            options={APPEARANCE_OPTIONS}
            label="Appearance"
            onChange={(m) => {
              applyAppearance(m);
              onAppearanceChange(m);
            }}
          />
        </div>
        <p className="mt-2 text-[11.5px] leading-relaxed text-muted">
          <span className="font-[510] text-ink-2">Auto</span> follows your system light/dark
          setting and switches live when the system does. Pinning a mode overrides it until you
          change it back.
        </p>
      </section>

      {/* ── Database ──────────────────────────────────────────────────────── */}
      <section className="card card-pad rise-in" style={{ "--i": 1 } as React.CSSProperties}>
        <h2 className="mb-3 flex items-center gap-2 text-[13px] font-[590] tracking-tight text-ink">
          <Database size={15} className="text-muted" strokeWidth={1.9} />
          Database
        </h2>
        {info.isLoading ? (
          <div className="grid grid-cols-2 gap-x-6 gap-y-2.5">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="flex items-baseline justify-between gap-3">
                <Skeleton h={9} w={72} />
                <Skeleton h={9} w={64} />
              </div>
            ))}
          </div>
        ) : (
          <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-[12.5px]">
            <Row label="Version" value={info.data?.version ?? "—"} />
            <Row label="Products" value={fmtNum(info.data?.products ?? 0)} />
            <Row label="Batches" value={fmtNum(info.data?.batches ?? 0)} />
            <Row label="Ledger rows" value={fmtNum(info.data?.ledgerRows ?? 0)} />
            <Row
              label="Full-text search"
              value={info.data?.fts5 ? "FTS5 enabled" : "Fallback (LIKE + trigram)"}
            />
            <Row label="Today" value={info.data?.today ?? "—"} />
          </dl>
        )}
        <div className="mt-3 flex items-center gap-2 rounded-[var(--radius-control)] border border-border bg-surface-2 px-2.5 py-2">
          <HardDrive size={13} className="shrink-0 text-faint" strokeWidth={1.9} />
          {info.isLoading ? (
            <Skeleton h={9} w="60%" />
          ) : (
            <code className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted">
              {info.data?.dbPath ?? "…"}
            </code>
          )}
        </div>
        <p className="mt-2 text-[11px] leading-snug text-faint">
          The ledger is append-only — corrections are recorded as adjustments rather than edits, so
          history is always reconstructable.
        </p>
      </section>

      {/* ── Model routing ─────────────────────────────────────────────────── */}
      <section className="card card-pad rise-in" style={{ "--i": 2 } as React.CSSProperties}>
        <div className="mb-1 flex items-center gap-2">
          <Cpu size={15} className="text-muted" strokeWidth={1.9} />
          <h2 className="title-tight text-[13px] font-[590] text-ink">Model routing</h2>
          <div className="flex-1" />
          <button
            onClick={() => unload.mutate()}
            disabled={unload.isPending || (h?.loaded.length ?? 0) === 0}
            className="btn btn-ghost !h-[26px] !px-2.5 !text-[11.5px]"
            title="Evict every resident model immediately"
          >
            {unload.isPending ? <Spinner size={12} /> : <Zap size={12.5} strokeWidth={1.9} />}
            Unload models
          </button>
        </div>
        <p className="mb-3 text-[11.5px] leading-relaxed text-muted">
          Each task is assigned the smallest model that reliably handles it. Cheap models stay warm
          for five minutes; expensive ones are evicted the moment they finish, so an idle app holds
          nothing in RAM.
        </p>

        {!h?.online && h ? (
          <div className="mb-3 rounded-[var(--radius-control)] border border-warn/30 bg-warn/8 px-3 py-2.5 text-[12px] leading-snug text-ink-2">
            <span className="font-[510] text-warn-ink">
              Ollama is not reachable at {h.host || "localhost"}.
            </span>{" "}
            Start it with{" "}
            <code className="rounded bg-surface-3 px-1 font-mono text-[11px] text-accent">
              ollama serve
            </code>
            . The app works without it in deterministic-only mode.
          </div>
        ) : null}

        <table className="tbl !text-[12px]">
          <thead>
            <tr>
              <th>Task</th>
              <th>Model</th>
              <th>Keep-alive</th>
              <th className="num">Size</th>
              <th>Override</th>
            </tr>
          </thead>
          {health.isLoading ? (
            <SkeletonRows rows={5} cols={5} align={["left", "left", "left", "right", "left"]} />
          ) : (
            <tbody>
              {(h?.routing ?? []).map((r) => (
                <tr key={r.task}>
                  <td className="font-[510] text-ink">{r.task}</td>
                  <td className="font-mono text-[11.5px] text-ink-2">{r.model}</td>
                  <td>
                    <span
                      className={`pill ${r.keepAlive === "0" ? "pill-ok" : "pill-neutral"}`}
                      title={
                        r.keepAlive === "0"
                          ? "Unloaded immediately after the response"
                          : "Held in memory for this long"
                      }
                    >
                      {r.keepAlive === "0" ? "unload" : r.keepAlive}
                    </span>
                  </td>
                  <td className="num tnum text-muted">{r.approxSize}</td>
                  <td>
                    <select
                      value={overrides.get(r.task) ?? ""}
                      onChange={(e) => setOverride.mutate({ task: r.task, model: e.target.value || null })}
                      className="input !h-[26px] !w-[184px] !text-[11.5px]"
                      aria-label={`Override model for ${r.task}`}
                    >
                      <option value="">Default</option>
                      {(h?.models ?? []).map((m) => (
                        <option key={m.name} value={m.name}>
                          {m.name} · {m.sizeGb} GB
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          )}
        </table>

        {h && h.loaded.length > 0 ? (
          <div className="mt-3 rounded-[var(--radius-control)] border border-ok/30 bg-ok/8 px-3 py-2">
            <div className="mb-1.5 text-[11px] font-[590] tracking-wider text-ok-ink uppercase">
              Currently resident
            </div>
            <div className="flex flex-wrap gap-1.5">
              {h.loaded.map((m) => (
                <span key={m.name} className="pill pill-ok">
                  {m.name} · {m.sizeGb} GB
                </span>
              ))}
            </div>
          </div>
        ) : health.isLoading ? null : (
          <p className="mt-3 text-[11.5px] text-faint">
            No model is resident. The agent answers rule-based questions without loading anything.
          </p>
        )}
      </section>

      {/* ── Current state ─────────────────────────────────────────────────── */}
      <section className="card card-pad rise-in" style={{ "--i": 3 } as React.CSSProperties}>
        <h2 className="mb-2 title-tight text-[13px] font-[590] text-ink">Current state</h2>
        {insights.isLoading ? (
          <div className="space-y-2">
            {Array.from({ length: 4 }).map((_, i) => (
              <div key={i} className="flex items-center justify-between gap-4">
                <Skeleton h={9} w={132} />
                <Skeleton h={9} w={168} />
                <Skeleton h={9} w={30} />
              </div>
            ))}
          </div>
        ) : (insights.data?.insights.length ?? 0) === 0 ? (
          <p className="text-[11.5px] text-faint">
            Nothing flagged. Analyses appear once there is stock and movement to examine.
          </p>
        ) : (
          <ul className="space-y-1">
            {insights.data?.insights.map((i) => (
              <li key={i.id} className="flex items-baseline justify-between gap-4 text-[12px]">
                <span className="text-muted">{i.title}</span>
                <span className="min-w-0 flex-1 truncate text-right text-[11.5px] text-faint">
                  {i.detail}
                </span>
                <span className="tnum w-[54px] text-right font-[510] text-ink">
                  {fmtNum(i.itemCount)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* ── Demo data ─────────────────────────────────────────────────────── */}
      <section className="card card-pad rise-in" style={{ "--i": 4 } as React.CSSProperties}>
        <h2 className="mb-1 title-tight text-[13px] font-[590] text-ink">Demo data</h2>
        <p className="mb-3 text-[11.5px] leading-relaxed text-muted">
          Rebuilding wipes every product, batch and ledger entry, then regenerates the 16-product
          catalogue with 120 days of sales history.
        </p>
        {!confirmReset ? (
          <button onClick={() => setConfirmReset(true)} className="btn btn-ghost">
            <RefreshCw size={13.5} strokeWidth={1.9} />
            Rebuild demo data
          </button>
        ) : (
          <div className="rounded-[var(--radius-control)] border border-danger/40 bg-danger/8 p-3">
            <p className="text-[12.5px] font-[510] text-danger-ink">
              This deletes all {fmtNum(info.data?.products ?? 0)} products and{" "}
              {fmtNum(info.data?.ledgerRows ?? 0)} ledger entries.
            </p>
            <div className="mt-2.5 flex gap-2">
              <button
                onClick={() => reseed.mutate()}
                disabled={reseed.isPending}
                className="btn btn-danger"
              >
                {reseed.isPending ? <Spinner size={13} /> : <Trash2 size={13.5} strokeWidth={1.9} />}
                Delete and rebuild
              </button>
              <button onClick={() => setConfirmReset(false)} className="btn btn-ghost">
                Cancel
              </button>
            </div>
          </div>
        )}
      </section>

      <p className="flex items-center gap-1.5 pb-2 text-[11px] text-faint">
        <Info size={11.5} strokeWidth={1.9} />
        Everything runs on this machine. No account, no telemetry, no network calls except to your
        local Ollama.
      </p>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-muted">{label}</dt>
      <dd className="tnum truncate font-[510] text-ink">{value}</dd>
    </div>
  );
}
