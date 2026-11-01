import { AlertTriangle, Check, Inbox, Info, Loader2, X } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { VelocityTracker, haptic, prefersReducedMotion } from "../lib/motion";

/* ── Toasts ──────────────────────────────────────────────────────────────────
   Small queue; errors always land here so nothing fails silently.
   Swipe to dismiss tracks the pointer 1:1, and the release *velocity* — not the
   release position — decides dismiss vs snap-back, so a fast flick works even
   short of the threshold. Enter and exit run the same path, mirrored. */

export type ToastKind = "ok" | "warn" | "error" | "info";
export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  detail?: string;
}

export function ToastHost({ toasts, dismiss }: { toasts: Toast[]; dismiss: (id: number) => void }) {
  const [closing, setClosing] = useState<Set<number>>(new Set());

  const beginDismiss = (id: number) => {
    if (closing.has(id)) return;
    setClosing((s) => new Set(s).add(id));
    window.setTimeout(() => dismiss(id), 190);
  };

  useEffect(() => {
    // Retire closing flags for toasts that already unmounted.
    setClosing((s) => {
      const ids = new Set(toasts.map((t) => t.id));
      const next = new Set([...s].filter((id) => ids.has(id)));
      return next.size === s.size ? s : next;
    });
  }, [toasts]);

  if (toasts.length === 0) return null;
  return (
    <div
      className="pointer-events-none fixed bottom-4 left-1/2 z-50 flex w-[min(520px,92vw)] -translate-x-1/2 flex-col gap-2"
      role="status"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <ToastItem
          key={t.id}
          toast={t}
          closing={closing.has(t.id)}
          onDismiss={() => beginDismiss(t.id)}
        />
      ))}
    </div>
  );
}

const TOAST_ICON: Record<ToastKind, { icon: typeof Check; className: string }> = {
  ok: { icon: Check, className: "text-ok-ink" },
  error: { icon: X, className: "text-danger-ink" },
  warn: { icon: AlertTriangle, className: "text-warn-ink" },
  info: { icon: Info, className: "text-accent" },
};

function ToastItem({
  toast: t,
  closing,
  onDismiss,
}: {
  toast: Toast;
  closing: boolean;
  onDismiss: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef({ startX: 0, dragging: false, tracker: new VelocityTracker() });

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
    drag.current = { startX: e.clientX, dragging: true, tracker: new VelocityTracker() };
    drag.current.tracker.push(e.clientX);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!drag.current.dragging) return;
    drag.current.tracker.push(e.clientX);
    const dx = e.clientX - drag.current.startX;
    // 10px hysteresis before committing to the gesture, so a sloppy click
    // doesn't nudge the toast.
    if (Math.abs(dx) < 10) return;
    const el = ref.current;
    if (el) {
      el.style.transform = `translateX(${dx}px)`;
      el.style.opacity = String(Math.max(0.35, 1 - Math.abs(dx) / 300));
    }
  };

  const endDrag = (e: React.PointerEvent) => {
    if (!drag.current.dragging) return;
    drag.current.dragging = false;
    const dx = e.clientX - drag.current.startX;
    const velocity = drag.current.tracker.velocity();
    const flick = Math.abs(velocity) > 600;
    const el = ref.current;

    if (Math.abs(dx) > 88 || flick) {
      const dir = flick ? Math.sign(velocity) || Math.sign(dx) || 1 : Math.sign(dx) || 1;
      haptic("tick");
      if (el) {
        el.style.transition = "transform 190ms cubic-bezier(0.65,0,0.68,0.28), opacity 190ms ease";
        el.style.transform = `translateX(${dir > 0 ? 320 : -320}px)`;
        el.style.opacity = "0";
      }
      window.setTimeout(onDismiss, 160);
      return;
    }

    // Snap back from wherever the finger left it — no brick wall.
    if (el) {
      el.style.transition = "transform 280ms cubic-bezier(0.32,0.72,0.35,1), opacity 180ms ease";
      el.style.transform = "";
      el.style.opacity = "";
      window.setTimeout(() => {
        if (ref.current) ref.current.style.transition = "";
      }, 300);
    }
  };

  const { icon: Icon, className: iconClass } = TOAST_ICON[t.kind];

  return (
    <div
      ref={ref}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      className={`glass-pop pointer-events-auto flex touch-pan-y items-start gap-2.5 rounded-[var(--radius-pop)] px-3.5 py-2.5 select-none ${
        closing ? "is-closing material-toast" : "material-toast"
      }`}
    >
      <span className="mt-px shrink-0" aria-hidden>
        <Icon size={14} strokeWidth={2.2} className={iconClass} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="vibrant text-[12.5px] text-ink">{t.title}</div>
        {t.detail ? (
          <div className="mt-0.5 leading-snug break-words text-[11.5px] text-muted">{t.detail}</div>
        ) : null}
      </div>
      <button
        onClick={onDismiss}
        className="-mr-0.5 shrink-0 rounded-md p-0.5 text-faint transition-colors hover:text-ink"
        aria-label="Dismiss notification"
      >
        <X size={13} />
      </button>
    </div>
  );
}

/* ── Modal ──────────────────────────────────────────────────────────────────
   A modal task: translucent surface over a dimming scrim, so the background
   reads as pushed back rather than replaced. Esc closes, click-outside closes,
   Tab is trapped, and focus lands on Close so there is always a visible exit. */

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
  const [visible, setVisible] = useState(open);
  const [closing, setClosing] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setVisible(true);
      setClosing(false);
      requestAnimationFrame(() => closeRef.current?.focus());
    } else if (visible) {
      setClosing(true);
      const t = window.setTimeout(() => {
        setVisible(false);
        setClosing(false);
      }, 200);
      return () => window.clearTimeout(t);
    }
    return undefined;
  }, [open, visible]);

  useEffect(() => {
    if (!visible || closing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key === "Tab") {
        const root = dialogRef.current;
        if (!root) return;
        const focusable = root.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length === 0) return;
        const first = focusable[0]!;
        const last = focusable[focusable.length - 1]!;
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [visible, closing, onClose]);

  if (!visible) return null;
  return (
    <div
      className={`scrim fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-[var(--scrim)] p-6 pt-[8vh] ${
        closing ? "is-closing" : ""
      }`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className={`glass-pop w-full overflow-hidden rounded-[var(--radius-pop)] ${
          closing ? "is-closing material-pop-center" : "material-pop-center"
        }`}
        style={{ maxWidth: width }}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <header className="flex items-start justify-between gap-4 border-b border-[var(--hairline)] px-5 py-3.5">
          <div className="min-w-0">
            <h2 className="title-tight vibrant text-[14.5px] text-ink">{title}</h2>
            {subtitle ? <p className="mt-0.5 truncate text-[12px] text-muted">{subtitle}</p> : null}
          </div>
          <button
            ref={closeRef}
            onClick={onClose}
            className="pressable -mr-1 shrink-0 rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
            aria-label="Close dialog"
          >
            <X size={15} />
          </button>
        </header>
        <div className="scroll-y max-h-[68vh] px-5 py-4">{children}</div>
        {footer ? (
          <footer className="flex items-center justify-end gap-2 border-t border-[var(--hairline)] px-5 py-3">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

/* ── Field: label bound to its control. Errors surface on blur, never per
      keystroke — validating mid-word tells people they're wrong before they've
      finished being right. */
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
        {required ? (
          <span className="ml-0.5 text-danger-ink" aria-hidden>
            *
          </span>
        ) : null}
      </label>
      {children}
      {error ? (
        <p className="mt-1 text-[11.5px] text-danger-ink" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="mt-1 text-[11.5px] text-faint">{hint}</p>
      ) : null}
    </div>
  );
}

/* ── Toggle chip. Multi-select filters, so these are independent toggles
      rather than one segmented control. */
export function SegTab({
  active,
  onClick,
  children,
  label,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
  label?: string;
}) {
  return (
    <button
      onClick={() => {
        haptic("tick");
        onClick();
      }}
      aria-pressed={active}
      aria-label={label}
      className={`seg-tab rounded-[var(--radius-control)] px-2.5 py-1 text-[12px] font-[510] ${
        active
          ? "bg-accent text-on-accent shadow-[var(--control-sheen)]"
          : "text-muted hover:bg-surface-2 hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

/* ── Segmented control. One choice of several: a track with the selection
      raised out of it, the way macOS and iOS draw it. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex gap-0.5 rounded-[9px] bg-surface-2 p-0.5"
    >
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            role="radio"
            aria-checked={on}
            onClick={() => {
              if (on) return;
              haptic("tick");
              onChange(o.value);
            }}
            className={`seg-tab rounded-[7px] px-3 py-[3px] text-[12px] font-[510] ${
              on
                ? "bg-surface text-ink shadow-[var(--shadow-raise)]"
                : "text-muted hover:text-ink"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/* ── Empty state: says what this screen is for and offers the action that
      fills it. Never a shrug. */
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
    <div className="page-enter flex flex-col items-center justify-center px-6 py-16 text-center">
      <div className="mb-3.5 flex h-11 w-11 items-center justify-center rounded-[11px] bg-surface-2 text-muted">
        {icon ?? <Inbox size={19} strokeWidth={1.75} />}
      </div>
      <div className="title-tight text-[14px] font-[590] text-ink">{title}</div>
      <p className="mt-1.5 max-w-sm text-[12.5px] leading-relaxed text-muted">{body}</p>
      {action ? <div className="mt-5">{action}</div> : null}
    </div>
  );
}

export function ErrorState({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="page-enter flex flex-col items-center justify-center px-6 py-14 text-center">
      <div className="mb-3.5 flex h-11 w-11 items-center justify-center rounded-[11px] bg-danger/12 text-danger-ink">
        <AlertTriangle size={19} strokeWidth={1.75} aria-hidden />
      </div>
      <div className="title-tight text-[14px] font-[590] text-ink">Could not load</div>
      <p className="mt-1.5 max-w-md text-[12.5px] leading-relaxed text-muted">{message}</p>
      {onRetry ? (
        <button onClick={onRetry} className="btn btn-ghost mt-5">
          Try again
        </button>
      ) : null}
    </div>
  );
}

export function Spinner({ size = 14, className = "" }: { size?: number; className?: string }) {
  return <Loader2 size={size} className={`animate-spin ${className}`} aria-hidden />;
}

/* ── Skeletons ───────────────────────────────────────────────────────────────
   Every skeleton mirrors the real layout it stands in for — same row heights,
   same column count, same paddings — so content arriving never shifts anything.
   A skeleton that guesses is worse than a spinner. */

export function Skeleton({
  w,
  h = 12,
  rounded = 5,
  className = "",
  style,
}: {
  w?: number | string;
  h?: number | string;
  rounded?: number;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div
      className={`skeleton ${className}`}
      style={{ width: w, height: h, borderRadius: rounded, ...style }}
      aria-hidden
    />
  );
}

/** Paragraph placeholder. The last line is short, the way real text wraps. */
export function SkeletonText({
  lines = 3,
  className = "",
}: {
  lines?: number;
  className?: string;
}) {
  const widths = ["100%", "96%", "88%", "92%", "70%"];
  return (
    <div className={`space-y-1.5 ${className}`} aria-hidden>
      {Array.from({ length: lines }).map((_, i) => (
        <Skeleton
          key={i}
          h={9}
          w={i === lines - 1 ? "62%" : widths[i % widths.length]}
        />
      ))}
    </div>
  );
}

/**
 * Table body placeholder. Row height matches `.tbl tbody td` exactly, and
 * numeric columns are right-aligned like the real ones, so the switch to data
 * moves nothing. Pass `align` to mark which columns are numeric.
 */
export function SkeletonRows({
  rows = 8,
  cols = 6,
  align = [],
}: {
  rows?: number;
  cols?: number;
  align?: ("left" | "right")[];
}) {
  const widths = [68, 44, 56, 48, 36, 52, 60, 40];
  return (
    <tbody aria-hidden>
      {Array.from({ length: rows }).map((_, r) => (
        <tr key={r}>
          {Array.from({ length: cols }).map((_, c) => (
            <td key={c}>
              <div className={align[c] === "right" ? "flex justify-end" : ""}>
                <Skeleton
                  h={9}
                  w={`${widths[(r + c) % widths.length]}%`}
                  // Stagger so the sweep reads as one surface loading, not
                  // eight independent bars pulsing in lockstep.
                  style={{ animationDelay: `${((r * cols + c) % 9) * 70}ms` }}
                />
              </div>
            </td>
          ))}
        </tr>
      ))}
    </tbody>
  );
}

/** A full table placeholder including its header, for page-level loading. */
export function SkeletonTable({
  columns,
  rows = 10,
}: {
  columns: { label: string; num?: boolean; width?: number }[];
  rows?: number;
}) {
  return (
    <table className="tbl">
      {columns.some((c) => c.width) ? (
        <colgroup>
          {columns.map((c, i) => (
            <col key={i} style={c.width ? { width: c.width } : undefined} />
          ))}
        </colgroup>
      ) : null}
      <thead>
        <tr>
          {columns.map((c) => (
            <th key={c.label} className={c.num ? "num" : ""}>
              {c.label}
            </th>
          ))}
        </tr>
      </thead>
      <SkeletonRows
        rows={rows}
        cols={columns.length}
        align={columns.map((c) => (c.num ? "right" : "left"))}
      />
    </table>
  );
}

/** Card placeholder: a heading line plus body lines inside the real card box. */
export function SkeletonCard({
  lines = 3,
  className = "",
}: {
  lines?: number;
  className?: string;
}) {
  return (
    <div className={`card card-pad ${className}`}>
      <Skeleton h={10} w={132} />
      <SkeletonText lines={lines} className="mt-3.5" />
    </div>
  );
}

/* ── Disclosure: animates its own height, so expanding reads as the section
      growing rather than the page jumping. Content stays mounted while
      collapsed so the collapse can animate too. */
export function Disclosure({ open, children }: { open: boolean; children: ReactNode }) {
  const wrap = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const mounted = useRef(false);

  useLayoutEffect(() => {
    const w = wrap.current;
    const i = inner.current;
    if (!w || !i) return undefined;

    // Don't animate the first paint: a section that starts open shouldn't
    // slide in on mount.
    if (!mounted.current || prefersReducedMotion()) {
      mounted.current = true;
      w.style.height = open ? "auto" : "0px";
      return undefined;
    }

    const target = i.offsetHeight;
    if (open) {
      w.style.height = "0px";
      void w.offsetHeight; // flush, so the transition has a start value
      w.style.height = `${target}px`;
      const done = () => {
        // Back to auto so nested content can grow afterwards.
        if (wrap.current) wrap.current.style.height = "auto";
      };
      w.addEventListener("transitionend", done, { once: true });
      return () => w.removeEventListener("transitionend", done);
    }

    w.style.height = `${target}px`;
    void w.offsetHeight;
    w.style.height = "0px";
    return undefined;
  }, [open]);

  return (
    <div
      ref={wrap}
      className="overflow-hidden"
      style={{ transition: "height 280ms cubic-bezier(0.32,0.72,0.35,1)" }}
      // Kept mounted so the collapse can animate, but removed from the tab
      // order and the accessibility tree while closed.
      inert={!open}
    >
      <div ref={inner}>{children}</div>
    </div>
  );
}

/**
 * A number that eases to its new value instead of snapping. Used on KPI tiles
 * where a background refetch would otherwise make figures flicker between
 * unrelated values with no sense of movement.
 */
export function AnimatedNumber({
  value,
  format,
  className = "",
}: {
  value: number;
  format: (v: number) => string;
  className?: string;
}) {
  const [shown, setShown] = useState(value);
  const current = useRef(value);
  const raf = useRef<number | null>(null);

  useEffect(() => {
    current.current = shown;
  }, [shown]);

  useEffect(() => {
    if (prefersReducedMotion()) {
      setShown(value);
      return undefined;
    }
    const from = current.current;
    if (from === value) return undefined;

    const start = performance.now();
    const duration = 480;
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      setShown(from + (value - from) * eased);
      if (t < 1) raf.current = requestAnimationFrame(tick);
      else raf.current = null;
    };
    raf.current = requestAnimationFrame(tick);
    return () => {
      if (raf.current !== null) cancelAnimationFrame(raf.current);
      raf.current = null;
    };
  }, [value]);

  return <span className={className}>{format(Math.round(shown))}</span>;
}

/** Bar meter. Scales on the compositor rather than animating width. */
export function Meter({
  value,
  max,
  tone = "accent",
}: {
  value: number;
  max: number;
  tone?: "accent" | "ok" | "warn" | "danger";
}) {
  const pct = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
  const colour =
    tone === "danger"
      ? "bg-danger"
      : tone === "warn"
        ? "bg-warn"
        : tone === "ok"
          ? "bg-ok"
          : "bg-accent";
  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-surface-3">
      <div
        className={`h-full w-full origin-left rounded-full ${colour}`}
        style={{
          transform: `scaleX(${pct})`,
          transition: "transform 420ms cubic-bezier(0.2,0,0,1)",
        }}
      />
    </div>
  );
}
