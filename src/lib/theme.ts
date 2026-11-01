/**
 * Appearance. Three states, like System Settings: follow the system, or pin
 * light/dark. "auto" adds no class and lets the `prefers-color-scheme` block in
 * styles.css decide; the pinned modes set a class that overrides it.
 */

export type Appearance = "auto" | "light" | "dark";

const KEY = "stockpile.appearance";

export function readAppearance(): Appearance {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark" || v === "auto") return v;
  } catch {
    /* private mode / storage disabled — fall through to auto */
  }
  return "auto";
}

export function applyAppearance(mode: Appearance): void {
  const root = document.documentElement;
  root.classList.toggle("theme-light", mode === "light");
  root.classList.toggle("theme-dark", mode === "dark");
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* best effort */
  }
  // Keep the webview chrome (and any OS-drawn scrollbar) in step.
  const dark = mode === "dark" || (mode === "auto" && systemPrefersDark());
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", dark ? "#141415" : "#f2f2f7");
}

export function systemPrefersDark(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return true;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

/** Re-apply on system changes so "auto" tracks the OS live, without a reload. */
export function watchSystemAppearance(onChange: () => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return () => undefined;
  }
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  mq.addEventListener("change", onChange);
  return () => mq.removeEventListener("change", onChange);
}

export const APPEARANCE_OPTIONS: { value: Appearance; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];
