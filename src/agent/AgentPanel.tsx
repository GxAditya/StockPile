import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  CornerDownLeft,
  Cpu,
  Eraser,
  Eye,
  Loader2,
  Square,
  Wrench,
  X,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, onAgentEvent } from "../lib/api";
import { errorCopy, useApp } from "../lib/ctx";
import { fmtNum } from "../lib/format";
import type { AgentEvent } from "../lib/types";

const SESSION = "default";

/** Monotonic transcript item id. Module scope so `appendAssistant` can mint ids too. */
let seq = 1;
const nextId = () => seq++;

/** One entry in the transcript. Tool cards and prose interleave in arrival order. */
type Item =
  | { id: number; kind: "user"; text: string }
  | { id: number; kind: "assistant"; text: string; streaming?: boolean }
  | {
      id: number;
      kind: "tool";
      tool: string;
      args: unknown;
      summary: string;
      rows?: Record<string, unknown>[] | null;
      status: "running" | "done";
    }
  | { id: number; kind: "confirm"; callId: string; tool: string; summary: string; state: "pending" | "committed" | "cancelled" }
  | { id: number; kind: "notice"; text: string }
  | { id: number; kind: "error"; text: string }
  | { id: number; kind: "route"; model: string; path: string; task: string };

export function AgentPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { toast, reportError } = useApp();
  const qc = useQueryClient();
  const [items, setItems] = useState<Item[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  
  const health = useQuery({
    queryKey: ["agent-health"],
    queryFn: api.agentHealth,
    enabled: open,
    refetchInterval: 10_000,
  });

  const confirm = useMutation({
    mutationFn: (callId: string) => api.agentConfirm(callId),
    onSuccess: (r) => {
      setItems((prev) =>
        prev.map((it) =>
          it.kind === "confirm" && it.state === "pending" ? { ...it, state: "committed" } : it,
        ),
      );
      toast("ok", "Inventory updated", r.summary);
      void qc.invalidateQueries();
    },
    onError: (e) => {
      const { title, body } = errorCopy(e);
      setItems((prev) => [...prev, { id: nextId(), kind: "error", text: `${title}: ${body}` }]);
    },
  });

  const cancelPending = useMutation({
    mutationFn: (callId: string) => api.agentCancel(callId),
    onSuccess: (_v, callId) => {
      setItems((prev) =>
        prev.map((it) =>
          it.kind === "confirm" && it.callId === callId ? { ...it, state: "cancelled" } : it,
        ),
      );
    },
    onError: (e) => reportError(e, "Could not cancel"),
  });

  // Subscribe once for the panel's lifetime; events are filtered by session.
  useEffect(() => {
    if (!open) return;
    let unlisten: (() => void) | undefined;
    let alive = true;
    onAgentEvent(({ sessionId, event }) => {
      if (!alive || sessionId !== SESSION) return;
      applyEvent(event);
    }).then((fn) => {
      if (alive) unlisten = fn;
      else fn();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
    // `applyEvent` is stable via useCallback with setState only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const applyEvent = useCallback((e: AgentEvent) => {
    setItems((prev) => {
      switch (e.kind) {
        case "text":
          return appendAssistant(prev, e.delta);
        case "toolCall":
          return [
            ...prev,
            {
              id: nextId(),
              kind: "tool",
              tool: e.tool,
              args: e.args,
              summary: e.summary,
              rows: e.rows,
              status: "done",
            } as Item,
          ];
        case "confirmation":
          return [
            ...prev,
            {
              id: nextId(),
              kind: "confirm",
              callId: e.callId,
              tool: e.tool,
              summary: e.summary,
              state: "pending",
            } as Item,
          ];
        case "committed":
          return [
            ...prev,
            { id: nextId(), kind: "notice", text: e.summary } as Item,
          ];
        case "route":
          return [
            ...prev,
            {
              id: nextId(),
              kind: "route",
              model: e.model,
              path: e.path,
              task: e.task,
            } as Item,
          ];
        case "notice":
          return [...prev, { id: nextId(), kind: "notice", text: e.message } as Item];
        case "error":
          return [...prev, { id: nextId(), kind: "error", text: e.message } as Item];
        case "done":
          setBusy(false);
          // Refresh every cached view: the agent may have written to the ledger.
          void qc.invalidateQueries();
          return closeStreaming(prev);
        default:
          return prev;
      }
    });
  }, [qc]);

  // Keep the newest content in view as tokens stream in.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [items]);

  useEffect(() => {
    if (open) requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  const send = async () => {
    const text = input.trim();
    if (!text || busy) return;
    setItems((prev) => [...prev, { id: nextId(), kind: "user", text }]);
    setInput("");
    setBusy(true);
    try {
      await api.agentChat(text, SESSION);
    } catch (e) {
      const { title, body } = errorCopy(e);
      setItems((prev) => [...prev, { id: nextId(), kind: "error", text: `${title}: ${body}` }]);
      setBusy(false);
    }
  };

  const stop = () => {
    void api.agentStop();
    setBusy(false);
  };

  const clear = () => {
    setItems([]);
    void api.agentClearHistory(SESSION);
  };

  const online = health.data?.online ?? false;
  const loadedCount = health.data?.loaded.length ?? 0;

  if (!open) return null;

  return (
    <aside className="flex w-[420px] shrink-0 flex-col border-l border-border bg-surface">
      {/* ── Header ─────────────────────────────────────────────────────────── */}
      <header className="flex h-[52px] shrink-0 items-center gap-2.5 border-b border-border px-3.5">
        <span className="grid h-6 w-6 place-items-center rounded-[7px] bg-gradient-to-br from-accent-soft to-cyan text-white">
          <Zap size={13} strokeWidth={2} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-semibold tracking-tight text-ink">Agent</div>
          <div className="truncate text-[10.5px] text-faint">
            {online ? `Ollama · ${fmtNum(loadedCount)} resident` : "Deterministic mode"}
          </div>
        </div>
        {/* Two adjacent controls that are both an ✕ are indistinguishable. "Clear transcript"
            gets its own glyph; only the panel-dismiss action is an ✕. */}
        <button
          onClick={clear}
          className="btn btn-quiet !h-[26px] !px-1.5"
          title="Clear conversation"
          aria-label="Clear conversation"
        >
          <Eraser size={13} />
        </button>
        <button
          onClick={onClose}
          className="btn btn-quiet !h-[26px] !px-1.5"
          title="Close agent panel"
          aria-label="Close agent panel"
        >
          <X size={13} />
        </button>
      </header>

      {!online ? (
        <div className="flex shrink-0 items-start gap-2 border-b border-warn/30 bg-warn/8 px-3.5 py-2.5">
          <AlertTriangle size={13} className="mt-0.5 shrink-0 text-warn" strokeWidth={1.75} />
          <div className="text-[11.5px] leading-snug text-ink-2">
            <span className="font-medium text-warn">Ollama is not reachable.</span> Direct lookups and
            rule-based commands still work; free-form analysis does not. Start it with{" "}
            <code className="rounded bg-surface-3 px-1 font-mono text-[10.5px] text-cyan">
              ollama serve
            </code>
          </div>
        </div>
      ) : null}

      {/* ── Transcript ─────────────────────────────────────────────────────── */}
      <div ref={scrollRef} className="scroll-y min-h-0 flex-1 px-3 py-3">
        {items.length === 0 ? (
          <AgentEmpty
            online={online}
            onPick={(ex) => {
              setInput(ex);
              inputRef.current?.focus();
            }}
          />
        ) : (
          <div className="space-y-2.5">
            {items.map((it) => (
              <ItemView
                key={it.id}
                item={it}
                busy={busy}
                onConfirm={(cid) => confirm.mutate(cid)}
                onCancel={(cid) => cancelPending.mutate(cid)}
                confirming={confirm.isPending || cancelPending.isPending}
              />
            ))}
          </div>
        )}
      </div>

      {/* ── Composer ───────────────────────────────────────────────────────── */}
      <div className="shrink-0 border-t border-border p-3">
        <div className="rounded-[11px] border border-border bg-canvas transition-colors focus-within:border-accent">
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
            rows={2}
            placeholder={
              online
                ? "Ask about stock, or say “received 40 amox batch B1 expiring 2027-03-01 at 12.50”…"
                : "Try “how many units of amoxicillin do I have”…"
            }
            className="max-h-[132px] w-full resize-none bg-transparent px-3 py-2.5 text-[13px] leading-relaxed outline-none placeholder:text-faint"
          />
          <div className="flex items-center gap-1.5 border-t border-border px-2 py-1.5">
            {busy ? (
              <button onClick={stop} className="btn btn-ghost !h-[26px] !px-2 !text-[11.5px]">
                <Square size={11} fill="currentColor" strokeWidth={0} />
                Stop
              </button>
            ) : null}
            <button
              onClick={() => void send()}
              disabled={busy || !input.trim()}
              className="btn btn-primary ml-auto !h-[26px] !px-2.5 !text-[11.5px]"
            >
              Send
              <CornerDownLeft size={11.5} strokeWidth={2} />
            </button>
          </div>
        </div>
        <p className="mt-1.5 text-center text-[10px] text-faint">
          Changes are previewed and need your confirmation. Enter to send · Shift+Enter for a new line
        </p>
      </div>
    </aside>
  );
}

function appendAssistant(prev: Item[], delta: string): Item[] {
  const last = prev[prev.length - 1];
  if (last && last.kind === "assistant") {
    const next = [...prev];
    next[next.length - 1] = { ...last, text: last.text + delta, streaming: true };
    return next;
  }
  return [
    ...prev,
    { id: nextId(), kind: "assistant", text: delta, streaming: true } as Item,
  ];
}

function closeStreaming(prev: Item[]): Item[] {
  return prev.map((it) => (it.kind === "assistant" ? { ...it, streaming: false } : it));
}

/**
 * Tool rows arrive as raw JSON, so a price is `12.678504067289196` and an ISO date wraps onto
 * three lines in a 60px column. Both make the grounding unreadable — which defeats the purpose
 * of showing it. Round floats to a sensible precision and keep dates on one line.
 */
function formatCell(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "number") {
    if (!Number.isFinite(v)) return String(v);
    // Big magnitudes are counts/currency; small ones are ratios and prices.
    return Number.isInteger(v)
      ? v.toLocaleString("en-IN")
      : Math.abs(v) >= 1
        ? v.toLocaleString("en-IN", { maximumFractionDigits: 2 })
        : v.toPrecision(3).replace(/0+$/, "").replace(/\.$/, "");
  }
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (typeof v === "object") return JSON.stringify(v);
  const s = String(v);
  // ISO date-time → date, so a timestamp does not wrap.
  const m = /^(\d{4}-\d{2}-\d{2})[T ]/.exec(s);
  return m?.[1] ?? s;
}

function rawCell(row: Record<string, unknown>, key: string): string {
  const v = row[key];
  return v === null || v === undefined ? "null" : String(v);
}

function ItemView({
  item,
  busy,
  onConfirm,
  onCancel,
  confirming,
}: {
  item: Item;
  busy: boolean;
  onConfirm: (callId: string) => void;
  onCancel: (callId: string) => void;
  confirming: boolean;
}) {
  switch (item.kind) {
    case "user":
      return (
        <div className="flex justify-end">
          <div className="max-w-[88%] rounded-[11px] rounded-br-[3px] bg-accent/20 px-3 py-2 text-[12.5px] leading-relaxed text-ink">
            {item.text}
          </div>
        </div>
      );

    case "assistant":
      if (!item.text.trim()) return null;
      return (
        <div className="max-w-[95%] text-[12.5px] leading-relaxed text-ink-2">
          <Markdown text={item.text} caret={item.streaming} />
        </div>
      );

    case "route":
      return (
        <div className="flex items-center gap-1.5 text-[10.5px] text-faint">
          <Cpu size={10.5} strokeWidth={1.75} />
          <span className="font-mono">{item.model}</span>
          <span>· {item.path}</span>
        </div>
      );

    case "notice":
      return (
        <div className="rounded-[9px] border border-border bg-surface-2 px-2.5 py-1.5 text-[11.5px] leading-snug text-muted">
          {item.text}
        </div>
      );

    case "error":
      return (
        <div className="rounded-[9px] border border-danger/35 bg-danger/8 px-2.5 py-1.5 text-[11.5px] leading-snug text-danger">
          {item.text}
        </div>
      );

    case "tool": {
      const columns = Object.keys(item.rows?.[0] ?? {});
      // No "ARGUMENTS" JSON dump. The summary line already states what was looked up, and the
      // raw argument object is developer noise in an operator's chat panel. The returned rows
      // stay, because those are the grounding.
      if (!item.rows || item.rows.length === 0) {
        return (
          <div className="flex items-start gap-2 rounded-[10px] border border-border bg-surface-2/60 px-2.5 py-2 text-[11.5px] leading-snug">
            <Wrench size={11.5} strokeWidth={1.75} className="mt-[3px] shrink-0 text-accent-soft" />
            <span className="text-ink-2">{item.summary}</span>
          </div>
        );
      }
      return (
        <details open className="group rounded-[10px] border border-border bg-surface-2/60">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-2.5 py-2 text-[11.5px] text-muted">
            <Wrench size={11.5} strokeWidth={1.75} className="shrink-0 text-accent-soft" />
            <span className="min-w-0 flex-1 text-ink-2">{item.summary}</span>
            {item.rows && item.rows.length > 0 ? (
              <span className="tnum shrink-0 rounded bg-surface-3 px-1.5 py-px text-[10px] text-faint">
                {item.rows.length} {item.rows.length === 1 ? "row" : "rows"}
              </span>
            ) : null}
          </summary>
          {item.rows && item.rows.length > 0 ? (
            <div className="scroll-y max-h-[220px] overflow-x-auto border-t border-border">
              <table className="tbl !text-[10.5px]">
                <thead className="sticky top-0">
                  <tr>
                    {columns.map((k) => (
                      <th key={k} className="!py-1 !px-2 !pl-2.5 !text-[9.5px] whitespace-nowrap">
                        {k.replace(/([a-z0-9])([A-Z])/g, "$1 $2")}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {item.rows.slice(0, 40).map((r, i) => (
                    <tr key={i}>
                      {columns.map((k) => (
                        <td
                          key={k}
                          className="tnum whitespace-nowrap !py-1 !px-2 !pl-2.5 text-[10.5px]"
                          title={rawCell(r, k)}
                        >
                          {formatCell(r[k])}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
        </details>
      );
    }

    case "confirm":
      return (
        <div
          className={`rounded-[10px] border px-2.5 py-2.5 ${
            item.state === "pending"
              ? "border-accent/50 bg-accent/8"
              : item.state === "committed"
                ? "border-ok/40 bg-ok/8"
                : "border-border bg-surface-2 opacity-70"
          }`}
        >
          <div className="mb-1.5 flex items-center gap-1.5 text-[10.5px] font-medium uppercase tracking-wider text-muted">
            {item.state === "pending" ? (
              <AlertTriangle size={11} className="text-accent-soft" strokeWidth={1.75} />
            ) : item.state === "committed" ? (
              <Check size={11} className="text-ok" strokeWidth={2} />
            ) : (
              <X size={11} strokeWidth={2} />
            )}
            {item.state === "pending"
              ? "Confirm this change"
              : item.state === "committed"
                ? "Applied"
                : "Cancelled"}
          </div>
          <div className="text-[12.5px] font-medium leading-snug text-ink">{item.summary}</div>
          {item.state === "pending" ? (
            <div className="mt-2.5 flex gap-1.5">
              <button
                onClick={() => onConfirm(item.callId)}
                disabled={confirming}
                className="btn btn-primary !h-[27px] !px-2.5 !text-[11.5px]"
              >
                {confirming ? (
                  <Loader2 size={12} className="animate-spin" />
                ) : (
                  <Check size={12.5} strokeWidth={2.2} />
                )}
                Confirm
              </button>
              <button
                onClick={() => onCancel(item.callId)}
                disabled={confirming || busy}
                className="btn btn-ghost !h-[27px] !px-2.5 !text-[11.5px]"
              >
                Cancel
              </button>
            </div>
          ) : null}
        </div>
      );

    default:
      return null;
  }
}

/** Minimal, safe renderer: bold, inline code, and newlines. No HTML injection path. */
function Markdown({ text, caret }: { text: string; caret?: boolean }) {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return (
    <p className={caret ? "caret" : undefined}>
      {parts.map((p, i) => {
        if (p.startsWith("**") && p.endsWith("**")) {
          return (
            <strong key={i} className="font-semibold text-ink">
              {p.slice(2, -2)}
            </strong>
          );
        }
        if (p.startsWith("`") && p.endsWith("`")) {
          return (
            <code key={i} className="rounded bg-surface-3 px-1 font-mono text-[11.5px] text-cyan">
              {p.slice(1, -1)}
            </code>
          );
        }
        return <span key={i}>{p}</span>;
      })}
    </p>
  );
}

function AgentEmpty({ online, onPick }: { online: boolean; onPick: (s: string) => void }) {
  const examples = online
    ? [
        "how many units of amoxicillin do I have",
        "what is expiring in the next 30 days",
        "received 40 amox batch B2401 expiring 2027-03-01 at 12.50",
        "sold 5 Panadol to Mr. Sharma",
        "show total inventory value by category",
      ]
    : [
        "how many units of amoxicillin do I have",
        "what is expiring in the next 30 days",
        "show total inventory value by category",
        "list low stock products",
      ];

  return (
    <div className="py-6">
      <div className="mb-1.5 flex items-center gap-2 text-[12.5px] font-medium text-ink">
        <Zap size={13} className="text-accent-soft" strokeWidth={1.75} />
        Ask about your stock
      </div>
      <p className="mb-4 text-[11.5px] leading-relaxed text-muted">
        Common questions and stock changes are answered by rules first, without loading a model. Only
        genuinely open-ended questions reach Ollama.
      </p>
      {/* These are the agent's hardest cases. Being able to fire one and watch it resolve is how
          you convince yourself the deterministic path works, so they are real buttons. */}
      <div className="space-y-1.5">
        {examples.map((ex) => (
          <button
            key={ex}
            type="button"
            onClick={() => onPick(ex)}
            className="block w-full rounded-[9px] border border-border bg-surface-2/50 px-2.5 py-1.5 text-left text-[11.5px] leading-snug text-ink-2 transition-colors hover:border-accent/50 hover:bg-surface-2 hover:text-ink"
          >
            {ex}
          </button>
        ))}
      </div>
      <p className="mt-4 flex items-center gap-1.5 text-[10.5px] text-faint">
        <Eye size={11} strokeWidth={1.75} />
        Answers are grounded in real rows. Every tool call is shown so you can verify it.
      </p>
    </div>
  );
}