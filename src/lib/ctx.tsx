import { createContext, useContext } from "react";
import type { ToastKind } from "../components/ui";
import { ApiError } from "./api";

export interface AppCtx {
  /** Raise a toast. The only way user-facing feedback is surfaced. */
  toast: (kind: ToastKind, title: string, detail?: string) => void;
  /** Normalise any thrown value into a toast. Every command failure funnels here. */
  reportError: (e: unknown, fallback?: string) => void;
}

export const Ctx = createContext<AppCtx>({
  toast: () => undefined,
  reportError: () => undefined,
});

export const useApp = () => useContext(Ctx);

/** Human-readable copy for each stable error code. Shown in modals where a toast is too fleeting. */
export function errorCopy(e: unknown): { title: string; body: string } {
  if (e instanceof ApiError) {
    switch (e.code) {
      case "SHORTFALL": {
        const requested = e.details?.requested;
        const available = e.details?.available;
        const product = e.details?.product;
        return {
          title: "Not enough stock",
          body: `${product ?? "That product"} has ${available ?? 0} unit(s) available but ${requested ?? "?"} were requested. Nothing was changed.`,
        };
      }
      case "CONFLICT":
        return { title: "Already exists", body: e.message };
      case "VALIDATION":
        return { title: "Check the details", body: e.message };
      case "NOT_FOUND":
        return { title: "Not found", body: e.message };
      case "AGENT":
        return { title: "Agent unavailable", body: e.message };
      default:
        return { title: "Something went wrong", body: e.message };
    }
  }
  return {
    title: "Something went wrong",
    body: e instanceof Error ? e.message : "Unexpected error.",
  };
}