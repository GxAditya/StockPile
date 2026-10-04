import { AlertTriangle, Check, Inbox, Loader2, X } from "lucide-react";
import type { ReactNode } from "react";

/* ── Toasts ──────────────────────────────────────────────────────────────────
   A small queue in `AppState` context; `useToast()` is the only way to raise
   one. Errors always land here so nothing fails silently. */
export type ToastKind = "ok" | "warn" | "error" | "info";
export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  detail?: string;
}

export function ToastHost({ toasts, dismiss }: { toasts: Toast[]; dismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div
      className="pointer-events-none fixed bottom-4 left-1/2 z-50 flex w-[min(560px,92vw)] -translate-x-1/2 flex-col gap-2"
      role="status"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`card pointer-events-auto flex items-start gap-3 px-4 py-3 shadow-2xl shadow-black/40 ${
            t.kind === "error" ? "border-danger/45" : t.kind === "warn" ? "border-warn/45" : ""
          }`}
        >
          <span className="mt-0.5 shrink-0">
            {t.kind === "ok" ? (
              <Check size={15} className="text-ok" />
            ) : t.kind === "error" ? (
              <X size={15} className="text-danger" />
            ) : t.kind === "warn" ? (
              <AlertTriangle size={15} className="text-warn" />
            ) : (
              <Check size={15} className="text-info" />
            )}
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-ink">{t.title}</div>
            {t.detail ? (
              <div className="mt-0.5 break-words text-[12px] leading-snug text-muted">{t.detail}</div>
            ) : null}
          </div>
          <button
            onClick={() => dismiss(t.id)}
            className="shrink-0 rounded p-0.5 text-faint transition-colors hover:text-ink"
            aria-label="Dismiss"
          >
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

/* ── Modal ────────────────────────────────────────────────────────────────── */
export function Modal({
  open,
  onClose,
  title,
  subtitle,
  children,
  footer,
  width = 560,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-black/62 p-6 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="card w-full shadow-2xl shadow-black/50"
        style={{ maxWidth: width }}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <header className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold tracking-tight text-ink">{title}</h2>
            {subtitle ? <p className="mt-0.5 text-[12px] text-muted">{subtitle}</p> : null}
          </div>
          <button
            onClick={onClose}
            className="-mr-1 shrink-0 rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </header>
        <div className="max-h-[68vh] overflow-y-auto px-5 py-4">{children}</div>
        {footer ? (
          <footer className="flex items-center justify-end gap-2 border-t border-border px-5 py-3.5">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

/* ── Field ────────────────────────────────────────────────────────────────── */
export function Field({
  label,
  hint,
  error,
  children,
  required,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  children: ReactNode;
  required?: boolean;
}) {
  return (
    <div>
      <label className="label">
        {label}
        {required ? <span className="ml-0.5 text-danger">*</span> : null}
      </label>
      {children}
      {error ? (
        <p className="mt-1 text-[11.5px] text-danger">{error}</p>
      ) : hint ? (
        <p className="mt-1 text-[11.5px] text-faint">{hint}</p>
      ) : null}
    </div>
  );
}

/* ── Empty state ────────────────────────────────────────────────────────────
   Always instructional with a primary action. Never a shrug. */
export function EmptyState({
  icon,
  title,
  body,
  action,
}: {
  icon?: ReactNode;
  title: string;
  body: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
      <div className="mb-3.5 flex h-11 w-11 items-center justify-center rounded-xl bg-surface-2 text-muted">
        {icon ?? <Inbox size={19} />}
      </div>
      <div className="text-[14px] font-medium text-ink">{title}</div>
      <p className="mt-1.5 max-w-sm text-[12.5px] leading-relaxed text-muted">{body}</p>
      {action ? <div className="mt-5">{action}</div> : null}
    </div>
  );
}

export function Spinner({ size = 15, className = "" }: { size?: number; className?: string }) {
  return <Loader2 size={size} className={`animate-spin ${className}`} />;
}

/** Table skeleton. Row count matches the real density so layout does not jump. */
export function SkeletonRows({ rows = 8, cols = 6 }: { rows?: number; cols?: number }) {
  return (
    <tbody>
      {Array.from({ length: rows }).map((_, r) => (
        <tr key={r}>
          {Array.from({ length: cols }).map((_, c) => (
            <td key={c}>
              <div
                className="skeleton h-3"
                style={{ width: `${[70, 45, 60, 50, 38, 55][c % 6]}%` }}
              />
            </td>
          ))}
        </tr>
      ))}
    </tbody>
  );
}

export function ErrorState({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
      <div className="mb-3.5 flex h-11 w-11 items-center justify-center rounded-xl bg-danger/10 text-danger">
        <AlertTriangle size={19} />
      </div>
      <div className="text-[14px] font-medium text-ink">Could not load</div>
      <p className="mt-1.5 max-w-md text-[12.5px] leading-relaxed text-muted">{message}</p>
      {onRetry ? (
        <button onClick={onRetry} className="btn btn-ghost mt-5">
          Try again
        </button>
      ) : null}
    </div>
  );
}

/** Zero-filled bar row used across KPI tiles. */
export function Meter({ value, max, tone = "accent" }: { value: number; max: number; tone?: string }) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  const colour =
    tone === "danger"
      ? "bg-danger"
      : tone === "warn"
        ? "bg-warn"
        : tone === "ok"
          ? "bg-ok"
          : "bg-accent-soft";
  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-surface-3">
      <div className={`h-full rounded-full ${colour}`} style={{ width: `${pct}%` }} />
    </div>
  );
}