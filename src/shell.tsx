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
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import { AgentPanel } from "./agent/AgentPanel";
import { api } from "./lib/api";
import { CommandPalette } from "./components/CommandPalette";
import { ToastHost } from "./components/ui";
import type { Toast } from "./components/ui";
import type { AgentHealth } from "./lib/types";

const NAV = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, key: "1" },
  { to: "/inventory", label: "Inventory", icon: Boxes, key: "2" },
  { to: "/sell", label: "Sell", icon: ShoppingCart, key: "3" },
  { to: "/history", label: "History", icon: HistoryIcon, key: "4" },
  { to: "/insights", label: "Insights", icon: Lightbulb, key: "5" },
];

const TITLES: Record<string, string> = {
  "/": "Dashboard",
  "/inventory": "Inventory",
  "/add": "Add stock",
  "/sell": "Sell stock",
  "/history": "History",
  "/insights": "Insights",
  "/settings": "Settings",
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

  // Global shortcuts. Page nav, palette, agent dock, dismiss.
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
      // Receiving stock is frequent enough to deserve a shortcut. Without one, the only route to
      // it was a sidebar click that silently did nothing.
      if (mod && e.key.toLowerCase() === "n") {
        e.preventDefault();
        nav("/add");
      }
      if (e.key === "Escape") {
        setPaletteOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [nav]);

  const openPalette = useCallback(() => setPaletteOpen(true), []);
  const title = TITLES[loc.pathname] ?? "StockPile";

  return (
    <div className="flex h-full w-full overflow-hidden bg-canvas">
      {/* ── Left rail ─────────────────────────────────────────────────────── */}
      <nav
        className={`flex shrink-0 flex-col border-r border-border bg-surface transition-[width] duration-200 ease-[cubic-bezier(0.2,0,0,1)] ${
          railOpen ? "w-[216px]" : "w-[60px]"
        }`}
      >
        <div className="flex h-[52px] items-center gap-2.5 border-b border-border px-3.5">
          <div className="grid h-7 w-7 shrink-0 place-items-center rounded-[8px] bg-gradient-to-br from-accent-soft to-cyan text-[13px] font-bold text-white">
            S
          </div>
          {railOpen ? (
            <div className="min-w-0">
              <div className="truncate text-[13px] font-semibold tracking-tight text-ink">
                StockPile
              </div>
              <div className="truncate text-[10.5px] text-faint">Local inventory</div>
            </div>
          ) : null}
        </div>

        <div className="flex flex-1 flex-col gap-0.5 p-2">
          {NAV.map((n) => {
            const Icon = n.icon;
            return (
              <NavLink
                key={n.to}
                to={n.to}
                end={n.to === "/"}
                className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
                title={railOpen ? undefined : n.label}
              >
                <Icon size={16.5} strokeWidth={1.75} className="shrink-0" />
                {railOpen ? <span className="truncate">{n.label}</span> : null}
              </NavLink>
            );
          })}

          <div className="my-2 h-px bg-border" />

          {/* Its own route. This used to point at `/sell?new=1`, which landed on the selling
              table with no effect — the receive dialog was never wired to that deep link. */}
          <NavLink
            to="/add"
            className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
            title={railOpen ? undefined : "Add stock"}
          >
            <PackagePlus size={16.5} strokeWidth={1.75} className="shrink-0" />
            {railOpen ? <span className="truncate">Add stock</span> : null}
          </NavLink>

          <div className="flex-1" />

          <NavLink to="/settings" className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}>
            <SettingsIcon size={16.5} strokeWidth={1.75} className="shrink-0" />
            {railOpen ? <span className="truncate">Settings</span> : null}
          </NavLink>

          <button
            onClick={() => setRailOpen((o) => !o)}
            className="btn btn-quiet mt-0.5 justify-start !px-2.5"
            aria-label={railOpen ? "Collapse sidebar" : "Expand sidebar"}
          >
            {railOpen ? <ChevronLeft size={15} /> : <ChevronRight size={15} />}
            {railOpen ? <span>Collapse</span> : null}
          </button>
        </div>
      </nav>

      {/* ── Main column ────────────────────────────────────────────────────── */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-[52px] shrink-0 items-center gap-3 border-b border-border bg-surface px-4">
          <h1 className="text-[14px] font-semibold tracking-tight text-ink">{title}</h1>
          <div className="flex-1" />

          <button
            onClick={openPalette}
            className="group flex h-[30px] w-[240px] items-center gap-2 rounded-lg border border-border bg-canvas px-2.5 text-left text-[12.5px] text-faint transition-colors hover:border-border-strong hover:text-muted"
          >
            <Search size={13.5} strokeWidth={1.75} />
            <span className="flex-1">Search products…</span>
            <span className="flex items-center gap-0.5 rounded border border-border bg-surface-2 px-1 py-px font-mono text-[10px] text-muted">
              <Command size={9} strokeWidth={2} />K
            </span>
          </button>

          <AgentStatusDot />

          <button
            onClick={() => setAgentOpen((o) => !o)}
            className={`btn ${agentOpen ? "btn-ghost" : "btn-primary"} !h-[30px] !px-2.5`}
            aria-pressed={agentOpen}
          >
            <Sparkles size={14.5} strokeWidth={1.75} />
            <span className={agentOpen ? "hidden xl:inline" : ""}>Agent</span>
          </button>
        </header>

        <main className="scroll-y min-h-0 flex-1">{children}</main>
      </div>

      {/* ── Agent dock ─────────────────────────────────────────────────────── */}
      <AgentPanel open={agentOpen} onClose={() => setAgentOpen(false)} />

      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      <ToastHost toasts={toasts} dismiss={dismiss} />
    </div>
  );
}

/** Live Ollama reachability plus a count of currently-resident models. */
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

  const label = !state.online ? "Ollama offline" : `${state.loaded} model${state.loaded === 1 ? "" : "s"} resident`;
  return (
    <span
      className="flex h-[30px] items-center gap-1.5 rounded-lg border border-border bg-canvas px-2.5 text-[11.5px] text-muted"
      title={state.online ? "Ollama reachable" : "Ollama not reachable — agent runs in deterministic-only mode"}
    >
      <span
        className={`h-[6px] w-[6px] rounded-full ${
          state.online ? "bg-ok shadow-[0_0_6px_var(--color-ok)]" : "bg-danger"
        }`}
      />
      <span className="tnum hidden lg:inline">{label}</span>
    </span>
  );
}