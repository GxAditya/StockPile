import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "./shell";
import { api, ApiError } from "./lib/api";
import { Ctx } from "./lib/ctx";
import type { Toast, ToastKind } from "./components/ui";
import { AddStock } from "./pages/AddStock";
import { Dashboard } from "./pages/Dashboard";
import { History } from "./pages/History";
import { Insights } from "./pages/Insights";
import { Inventory } from "./pages/Inventory";
import { Sell } from "./pages/Sell";
import { Settings } from "./pages/Settings";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Local SQLite reads are fast and the data only changes through this app, so a short stale
      // window avoids refetching on every panel toggle.
      staleTime: 5_000,
      gcTime: 5 * 60_000,
      refetchOnWindowFocus: false,
      retry: (count, err) => {
        // Validation, shortfall and conflict errors are deterministic; retrying cannot help.
        if (
          err instanceof ApiError &&
          ["VALIDATION", "SHORTFALL", "CONFLICT", "NOT_FOUND"].includes(err.code)
        ) {
          return false;
        }
        return count < 2;
      },
    },
  },
});

function Root() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  const toast = useCallback(
    (kind: ToastKind, title: string, detail?: string) => {
      const id = nextId.current++;
      setToasts((t) => [...t.slice(-3), { id, kind, title, detail }]);
      // Errors linger because they usually carry something to act on.
      window.setTimeout(() => dismiss(id), kind === "error" ? 9000 : 4200);
    },
    [dismiss],
  );

  const reportError = useCallback(
    (e: unknown, fallback = "Something went wrong") => {
      if (e instanceof ApiError) {
        toast("error", e.message, e.code === "SHORTFALL" ? undefined : e.code);
      } else {
        toast("error", fallback, e instanceof Error ? e.message : undefined);
      }
    },
    [toast],
  );

  const ctx = useMemo<AppCtxValue>(() => ({ toast, reportError }), [toast, reportError]);

  useEffect(() => {
    // A first run with an empty catalogue is unusable until it has data, so seed once.
    let cancelled = false;
    api
      .appInfo()
      .then((info) => {
        if (cancelled) return undefined;
        if (info.products === 0) {
          return api.seedDemo().then((r) => {
            if (cancelled) return;
            toast(
              "info",
              "Demo catalogue loaded",
              `${r.products} products · ${r.batches} batches · ${r.sales} sales`,
            );
            void queryClient.invalidateQueries();
          });
        }
        return undefined;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [toast]);

  return (
    <Ctx.Provider value={ctx}>
      <AppShell toasts={toasts} dismiss={dismiss}>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/inventory" element={<Inventory />} />
          <Route path="/add" element={<AddStock />} />
          <Route path="/sell" element={<Sell />} />
          <Route path="/history" element={<History />} />
          <Route path="/insights" element={<Insights />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </AppShell>
    </Ctx.Provider>
  );
}

interface AppCtxValue {
  toast: (kind: ToastKind, title: string, detail?: string) => void;
  reportError: (e: unknown, fallback?: string) => void;
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <HashRouter>
        <Root />
      </HashRouter>
    </QueryClientProvider>
  </StrictMode>,
);