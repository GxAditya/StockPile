import {
  Boxes,
  ChevronLeft,
  ChevronRight,
  Command,
  History as HistoryIcon,
  LayoutDashboard,
  Lightbulb,
  PackagePlus,
  Search,
  Settings as SettingsIcon,
  ShoppingCart,
  Sparkles,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import { AgentPanel } from "./agent/AgentPanel";
import { api } from "./lib/api";
import { haptic } from "./lib/motion";
import { CommandPalette } from "./components/CommandPalette";
import { ToastHost } from "./components/ui";
import type { Toast } from "./components/ui";
import type { AgentHealth } from "./lib/types";

const NAV = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, key: "1", hint: "KPIs · attention · charts" },
  { to: "/inventory", label: "Inventory", icon: Boxes, key: "2", hint: "Stock by product + batch" },
  { to: "/sell", label: "Sell", icon: ShoppingCart, key: "3", hint: "FEFO sale with preview" },
  { to: "/history", label: "History", icon: HistoryIcon, key: "4", hint: "Append-only ledger" },
  { to: "/insights", label: "Insights", icon: Lightbulb, key: "5", hint: "Deterministic analysis" },
];

/* Every screen answers: where am I, what's here, how do I get out. */
const TITLES: Record<string, { title: string; sub: string }> = {
  "/": { title: "Dashboard", sub: "Needs a decision first" },
  "/inventory": { title: "Inventory", sub: "Click a row for batches" },
  "/add": { title: "Add stock", sub: "Receive a batch" },
  "/sell": { title: "Sell stock", sub: "Earliest expiry first" },
  "/history": { title: "History", sub: "Every movement, newest first" },
  "/insights": { title: "Insights", sub: "SQL only · no model" },
  "/settings": { title: "Settings", sub: "Appearance · models · data" },
};

export function AppShell({
  toasts,
  dismiss,
  children,
}: {
  toasts: Toast[];
  dismiss: (id: number) => void;
  children: ReactNode;
}) {
  const [railOpen, setRailOpen] = useState(true);
  const [agentOpen, setAgentOpen] = useState(true);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const loc = useLocation();
  const nav = useNavigate();
  const mainRef = useRef<HTMLElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen(true);
        return;
      }
      if (mod && e.key.toLowerCase() === "j") {
        e.preventDefault();
        haptic("tick");
        setAgentOpen((o) => !o);
        return;
      }
      if (mod && e.key.toLowerCase() === "b") {
        e.preventDefault();
        setRailOpen((o) => !o);
        return;
      }
      if (mod && /^[1-5]$/.test(e.key)) {
        e.preventDefault();
        nav(NAV[Number(e.key) - 1]!.to);
      }
      if (mod && e.key.toLowerCase() === "n") {
        e.preventDefault();
        nav("/add");
      }
      if (e.key === "Escape" && !mod) {
        // The palette owns Esc while open; dialogs own it otherwise. The dock
        // deliberately stays — Esc should never be the thing that loses work.
        setPaletteOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [nav]);

  // A new screen starts at the top. Carrying the previous screen's scroll
  // offset into a different dataset is disorienting.
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [loc.pathname]);

  const openPalette = useCallback(() => setPaletteOpen(true), []);
  const meta = TITLES[loc.pathname] ?? { title: "StockPile", sub: "" };

  return (
    <div className="flex h-full w-full overflow-hidden bg-canvas">
      {/* ── Source list. Structural chrome, so it carries the heavier material. */}
      <nav
        aria-label="Primary"
        className={`glass-rail flex shrink-0 flex-col border-r border-[var(--hairline)] ${
          railOpen ? "w-[212px]" : "w-[56px]"
        }`}
        style={{ transition: "width 280ms cubic-bezier(0.32,0.72,0.35,1)" }}
      >
        <div className="flex h-[50px] items-center gap-2.5 px-3">
          <div
            className="grid h-[26px] w-[26px] shrink-0 place-items-center rounded-[7px] bg-accent text-[12.5px] font-bold text-on-accent shadow-[var(--control-sheen)]"
            aria-hidden
          >
            S
          </div>
          {railOpen ? (
            <div className="min-w-0">
              <div className="title-tight truncate text-[12.5px] font-[590] text-ink">
                StockPile
              </div>
              <div className="truncate text-[10.5px] text-faint">Local inventory</div>
            </div>
          ) : null}
        </div>

        <div className="flex flex-1 flex-col gap-px p-2">
          {NAV.map((n) => {
            const Icon = n.icon;
            return (
              <NavLink
                key={n.to}
                to={n.to}
                end={n.to === "/"}
                className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
                title={railOpen ? `${n.label} (⌘${n.key})` : `${n.label} (⌘${n.key}) — ${n.hint}`}
              >
                <Icon size={15.5} strokeWidth={1.9} className="shrink-0" aria-hidden />
                {railOpen ? <span className="min-w-0 flex-1 truncate">{n.label}</span> : null}
              </NavLink>
            );
          })}

          <div className="my-2 h-px bg-[var(--hairline)]" aria-hidden />

          <NavLink
            to="/add"
            className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
            title={railOpen ? "Add stock (⌘N)" : "Add stock (⌘N) — Receive a batch"}
          >
            <PackagePlus size={15.5} strokeWidth={1.9} className="shrink-0" aria-hidden />
            {railOpen ? <span className="truncate">Add stock</span> : null}
          </NavLink>

          <div className="flex-1" />

          <NavLink
            to="/settings"
            className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
            title={railOpen ? undefined : "Settings — Appearance · models · data"}
          >
            <SettingsIcon size={15.5} strokeWidth={1.9} className="shrink-0" aria-hidden />
            {railOpen ? <span className="truncate">Settings</span> : null}
          </NavLink>

          <button
            onClick={() => setRailOpen((o) => !o)}
            className="btn btn-quiet mt-0.5 justify-start !px-2.5"
            aria-label={railOpen ? "Collapse sidebar" : "Expand sidebar"}
            aria-expanded={railOpen}
          >
            {railOpen ? <ChevronLeft size={14.5} /> : <ChevronRight size={14.5} />}
            {railOpen ? <span>Collapse</span> : null}
          </button>
        </div>
      </nav>

      {/* ── Main column ────────────────────────────────────────────────── */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="glass-bar flex h-[50px] shrink-0 items-center gap-3 px-4">
          <div className="min-w-0">
            <h1 className="title-tight truncate text-[13.5px] font-[590] text-ink">{meta.title}</h1>
            {meta.sub ? (
              <p className="truncate text-[11px] leading-tight text-faint">{meta.sub}</p>
            ) : null}
          </div>
          <div className="flex-1" />

          <button
            onClick={openPalette}
            className="pressable flex h-[28px] w-[228px] items-center gap-2 rounded-[var(--radius-control)] border border-border bg-surface px-2.5 text-left text-[12px] text-faint shadow-[var(--shadow-raise)] transition-colors hover:border-border-strong hover:text-muted"
            aria-label="Search products or jump to a page (⌘K)"
          >
            <Search size={13} strokeWidth={1.9} aria-hidden />
            <span className="flex-1">Search…</span>
            <kbd className="kbd flex items-center gap-0.5">
              <Command size={9} strokeWidth={2.2} aria-hidden />K
            </kbd>
          </button>

          <AgentStatusDot />

          <button
            onClick={() => {
              haptic("tick");
              setAgentOpen((o) => !o);
            }}
            className={`btn ${agentOpen ? "btn-ghost" : "btn-primary"} !h-[28px] !px-2.5`}
            aria-pressed={agentOpen}
            aria-label={agentOpen ? "Close agent panel (⌘J)" : "Open agent panel (⌘J)"}
          >
            <Sparkles size={13.5} strokeWidth={1.9} aria-hidden />
            <span className={agentOpen ? "hidden xl:inline" : ""}>Agent</span>
          </button>
        </header>

        <main ref={mainRef} className="scroll-y relative min-h-0 flex-1">
          {/* Keyed on the path so each screen plays its entrance once. Query
              changes (?focus=…) deliberately don't re-key — that would throw
              away the screen the user is already reading. */}
          <div key={loc.pathname} className="page-enter h-full">
            {children}
          </div>
        </main>
      </div>

      {/* ── Agent dock: a parallel panel, so translucency and no scrim. ── */}
      <AgentPanel open={agentOpen} onClose={() => setAgentOpen(false)} />

      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      <ToastHost toasts={toasts} dismiss={dismiss} />
    </div>
  );
}

/** Live Ollama reachability plus resident-model count. */
function AgentStatusDot() {
  const [state, setState] = useState<{ online: boolean; loaded: number }>({
    online: false,
    loaded: 0,
  });

  useEffect(() => {
    let alive = true;
    const poll = () => {
      api
        .agentHealth()
        .then((h: AgentHealth) => {
          if (alive) setState({ online: h.online, loaded: h.loaded.length });
        })
        .catch(() => {
          if (alive) setState({ online: false, loaded: 0 });
        });
    };
    poll();
    const t = window.setInterval(poll, 8000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  const label = !state.online
    ? "Ollama offline"
    : `${state.loaded} model${state.loaded === 1 ? "" : "s"} resident`;
  return (
    <span
      className="flex h-[28px] items-center gap-1.5 rounded-[var(--radius-control)] border border-border bg-surface px-2.5 text-[11.5px] text-muted shadow-[var(--shadow-raise)]"
      title={
        state.online
          ? "Ollama reachable"
          : "Ollama not reachable — the agent runs in deterministic-only mode"
      }
      role="status"
    >
      <span
        className={`h-[6px] w-[6px] rounded-full ${state.online ? "bg-ok" : "bg-danger"}`}
        aria-hidden
      />
      <span className="tnum hidden lg:inline">{label}</span>
    </span>
  );
}
