/**
 * Apple fluid-interface motion primitives.
 * Translated from WWDC18 "Designing Fluid Interfaces" + audio-haptic talks.
 *
 * - §4 springs over fixed-duration. Default critically damped (1.0, no bounce).
 *   Bounce only when the gesture carried momentum.
 * - §5 velocity handoff: animation continues at the finger's exact velocity.
 * - §6 momentum projection: snap to target nearest *projected* endpoint.
 * - §9 rubber-banding past boundaries.
 * - §13 multimodal: visual + haptic fire on the same frame, only where earned.
 * - §3 interruptible: always start from live presentation value.
 */

export const SPRINGS = {
  default: { damping: 1.0, response: 0.4, bounce: 0 },
  sheet: { damping: 0.8, response: 0.3, bounce: 0.2 },
  momentum: { damping: 0.8, response: 0.4, bounce: 0.2 },
  press: { duration: 0.1 },
} as const;

export function project(initialVelocity: number, decelerationRate = 0.998): number {
  return ((initialVelocity / 1000) * decelerationRate) / (1 - decelerationRate);
}

export function projectedEndpoint(
  currentPosition: number,
  releaseVelocity: number,
  decelerationRate = 0.998,
): number {
  return currentPosition + project(releaseVelocity, decelerationRate);
}

export function relativeVelocity(
  gestureVelocity: number,
  currentValue: number,
  targetValue: number,
): number {
  const remaining = targetValue - currentValue;
  if (remaining === 0) return 0;
  return gestureVelocity / remaining;
}

export function rubberband(overshoot: number, dimension: number, constant = 0.55): number {
  return (
    (overshoot * dimension * constant) / (dimension + constant * Math.abs(overshoot))
  );
}

export function rubberbandClamp(
  value: number,
  min: number,
  max: number,
  dimension: number,
): number {
  if (value < min) return min + rubberband(value - min, dimension);
  if (value > max) return max + rubberband(value - max, dimension);
  return value;
}

/** Nearest snap point to the *projected* endpoint — not the release point (§6). */
export function nearestSnap(
  current: number,
  velocity: number,
  snaps: number[],
  decelerationRate = 0.998,
): number {
  if (snaps.length === 0) return current;
  const endpoint = projectedEndpoint(current, velocity, decelerationRate);
  let best = snaps[0]!;
  let bestDist = Math.abs(endpoint - best);
  for (const s of snaps.slice(1)) {
    const d = Math.abs(endpoint - s);
    if (d < bestDist) {
      bestDist = d;
      best = s;
    }
  }
  return best;
}

export class VelocityTracker {
  private samples: { pos: number; t: number }[] = [];

  push(pos: number, t = performance.now()): void {
    this.samples.push({ pos, t });
    while (this.samples.length > 2 && t - this.samples[0]!.t > 120) {
      this.samples.shift();
    }
    if (this.samples.length > 12) this.samples.shift();
  }

  velocity(): number {
    const s = this.samples;
    if (s.length < 2) return 0;
    const first = s[0]!;
    const last = s[s.length - 1]!;
    const dt = (last.t - first.t) / 1000;
    if (dt <= 0) return 0;
    return (last.pos - first.pos) / dt;
  }

  reset(): void {
    this.samples = [];
  }
}

export function springEasing(kind: "default" | "sheet" | "momentum" = "default"): string {
  switch (kind) {
    case "sheet":
      return "cubic-bezier(0.32, 0.72, 0.35, 1)";
    case "momentum":
      return "cubic-bezier(0.3, 0.9, 0.35, 1.05)";
    default:
      return "cubic-bezier(0.2, 0, 0, 1)";
  }
}

export function springDuration(kind: "default" | "sheet" | "momentum" = "default"): number {
  return kind === "sheet" ? 300 : 400;
}

/**
 * Haptic tick on the causal frame (§13). Harmony = visual + haptic same frame.
 * Utility = only where earned: commit, snap, error. No-op where unsupported.
 * Respects reduced-motion? Haptics are non-vestibular, so they stay — but keep
 * them short and sparse so users don't learn to ignore them.
 */
export function haptic(kind: "tick" | "commit" | "error" = "tick"): void {
  try {
    const nav = navigator as Navigator & { vibrate?: (p: number | number[]) => boolean };
    if (typeof nav.vibrate !== "function") return;
    if (kind === "commit") nav.vibrate(12);
    else if (kind === "error") nav.vibrate([18, 40, 18]);
    else nav.vibrate(6);
  } catch {
    /* vibration is best-effort */
  }
}

/** True when the user asked for gentler, non-vestibular feedback (§14). */
export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Decide reverse-vs-commit at release by velocity *sign*, not position.
 * A flick back toward origin reverses even if past midpoint; a fling forward
 * commits even if short of it. Falls back to position when velocity is ~0.
 */
export function shouldCommitByVelocity(
  velocity: number,
  current: number,
  threshold: number,
  velocityDeadzone = 80,
): boolean {
  if (Math.abs(velocity) > velocityDeadzone) return velocity > 0;
  return current > threshold;
}
