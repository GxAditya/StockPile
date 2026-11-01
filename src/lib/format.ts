import type { Health, Severity } from "./types";

const currency = new Intl.NumberFormat(undefined, {
  style: "currency",
  currency: "INR",
  maximumFractionDigits: 0,
});
const currencyPrecise = new Intl.NumberFormat(undefined, {
  style: "currency",
  currency: "INR",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});
const plain = new Intl.NumberFormat();

export const fmtMoney = (v: number | null | undefined, precise = false) => {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  return precise || Math.abs(v) < 1000 ? currencyPrecise.format(v) : currency.format(v);
};

export const fmtNum = (v: number | null | undefined) =>
  v === null || v === undefined || Number.isNaN(v) ? "—" : plain.format(v);

export const fmtSigned = (v: number) => (v > 0 ? `+${plain.format(v)}` : plain.format(v));

export function fmtDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso.length <= 10 ? `${iso}T00:00:00` : iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" });
}

export function fmtDateShort(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso.length <= 10 ? `${iso}T00:00:00` : iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(undefined, { day: "2-digit", month: "short" });
}

export function fmtDateTime(ts: string | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString(undefined, {
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function fmtTime(ts: string | null | undefined): string {
  if (!ts) return "";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** "3d ago" / "in 2h" — relative to now, compact enough for a dense table. */
export function fmtRelative(ts: string | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  const secs = (d.getTime() - Date.now()) / 1000;
  const abs = Math.abs(secs);
  const units: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "second"],
    [3600, "minute"],
    [86400, "hour"],
    [604800, "day"],
    [2629800, "week"],
    [31557600, "month"],
  ];
  let chosen: [number, Intl.RelativeTimeFormatUnit] = [1, "second"];
  for (const u of units) {
    if (abs >= u[0]) chosen = u;
  }
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  return rtf.format(Math.round(secs / chosen[0]), chosen[1]);
}

/** Days from today until a date. Negative when already past. */
export function daysUntil(iso: string | null | undefined): number | null {
  if (!iso) return null;
  const d = new Date(`${iso.slice(0, 10)}T00:00:00`);
  if (Number.isNaN(d.getTime())) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((d.getTime() - today.getTime()) / 86400000);
}

/**
 * Expiry urgency bucket. Used to colour-code a date; the accompanying pill always repeats the
 * meaning in words so colour is never the only signal.
 */
export function expiryTone(iso: string | null | undefined): {
  tone: "danger" | "warn" | "info" | "muted";
  label: string;
} {
  const d = daysUntil(iso);
  if (d === null) return { tone: "muted", label: "No expiry" };
  if (d < 0) return { tone: "danger", label: `Expired ${Math.abs(d)}d ago` };
  if (d === 0) return { tone: "danger", label: "Expires today" };
  if (d <= 7) return { tone: "danger", label: `${d}d left` };
  if (d <= 30) return { tone: "warn", label: `${d}d left` };
  if (d <= 90) return { tone: "info", label: `${d}d left` };
  return { tone: "muted", label: `${d}d left` };
}

export const HEALTH_META: Record<Health, { label: string; tone: string; glyph: string }> = {
  ok: { label: "OK", tone: "pill-ok", glyph: "●" },
  low: { label: "Low", tone: "pill-warn", glyph: "▼" },
  out: { label: "Out", tone: "pill-danger", glyph: "○" },
  expiring: { label: "Expiring", tone: "pill-warn", glyph: "◐" },
  expired: { label: "Expired", tone: "pill-danger", glyph: "✕" },
};

export const SEVERITY_META: Record<Severity, { tone: string; label: string }> = {
  critical: { tone: "pill-danger", label: "Critical" },
  warning: { tone: "pill-warn", label: "Warning" },
  info: { tone: "pill-info", label: "Info" },
};

export const KIND_META: Record<string, { label: string; tone: string }> = {
  restock: { label: "Restock", tone: "pill-info" },
  sale: { label: "Sale", tone: "pill-ok" },
  adjustment: { label: "Adjustment", tone: "pill-warn" },
  void: { label: "Void", tone: "pill-neutral" },
};

export const TONE_TEXT: Record<string, string> = {
  danger: "text-danger-ink",
  warn: "text-warn-ink",
  info: "text-info-ink",
  muted: "text-muted",
};

export const HEALTH_ORDER: { value: string; label: string }[] = [
  { value: "all", label: "All health" },
  { value: "expired", label: "Expired" },
  { value: "expiring", label: "Expiring ≤30d" },
  { value: "out", label: "Out of stock" },
  { value: "low", label: "Low" },
  { value: "ok", label: "OK" },
];