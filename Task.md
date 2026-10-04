# StockPile — Task Plan

**Date:** 2026-10-04
**Stack (decided in PRD §4):** Tauri v2 · React 19 · TypeScript · Vite · Tailwind v4 · SQLite (`rusqlite`) · Ollama
**Rule:** each task is independently verifiable. `buildlog.md` is updated after every task closes.

Status legend: `[ ]` pending · `[~]` in progress · `[x]` done · `[!]` blocked

---

## Phase 0 — Foundation

- [x] **T0.1** Verify toolchain and prerequisites (Rust, Node, Ollama, `webkit2gtk-4.1`, `gtk+-3.0`)
- [x] **T0.2** Scaffold project: `package.json`, `vite.config.ts`, `tsconfig.json`, Tailwind v4, `src-tauri/`
- [x] **T0.3** Build the design system: colour tokens, `@theme`, base CSS, typography, utility primitives
- [x] **T0.4** App shell: left nav rail, top bar, right agent dock slot, router with 6 routes
- [x] **T0.5** Rust↔TS IPC plumbing: typed command wrapper, `AppError` → UI toast mapping

---

## Phase 1 — Data Layer

- [x] **T1.1** SQLite connection: WAL, `foreign_keys`, `busy_timeout`, `query_only` read connection
- [x] **T1.2** Schema + migrations: `products`, `batches`, `ledger`, indices, FTS5 `products_fts` + triggers
- [x] **T1.3** Domain types (`Product`, `Batch`, `LedgerEntry`, `StockRow`) in both Rust and TS
- [x] **T1.4** Product repository: CRUD, FTS5 search, trigram fuzzy match
- [x] **T1.5** Batch repository: insert, list by product, live-batch query ordered by expiry
- [x] **T1.6** Ledger repository: append, filtered + paginated query, per-day subtotals, running balance
- [x] **T1.7** Seed command with realistic pharma/FMCG demo data across batches, expiries, buyers

---

## Phase 2 — Inventory Core (must-have features)

- [x] **T2.1** FEFO allocation engine + `sell_stock` transaction (multi-batch, ledger per batch, rollback)
- [x] **T2.2** `restock` / `adjust_stock` transactions with single-transaction ledger writes
- [x] **T2.3** Inventory stock display: product rows with computed health pill, totals, weighted-average cost, earliest expiry
- [x] **T2.4** Batch drill-down table with expiry colour buckets
- [x] **T2.5** Search / category / health filters + column sort + CSV export of filtered view
- [x] **T2.6** Add Stock flow: new-product-or-existing, live typeahead, inline validation
- [x] **T2.7** Sell flow: buyer capture, FEFO preview before commit, shortfall guard, expired override

---

## Phase 3 — History

- [x] **T3.1** History timeline grouped by day, sticky date headers, per-day subtotals, running balance
- [x] **T3.2** Sale rows show buyer name prominently; kind badges
- [x] **T3.3** Filters: kind, product, buyer, date range, free text
- [x] **T3.4** CSV export matching the filtered slice exactly

---

## Phase 4 — Dashboard & Insights

- [x] **T4.1** `dashboard_summary` command: 8 KPIs + 4 chart series in one round trip
- [x] **T4.2** KPI tile row with delta indicators
- [x] **T4.3** Charts: value-by-category donut, 14-day inbound/outbound bars, top-5 sold, expiry horizon
- [x] **T4.4** "Needs attention" live list of expired / expiring batches with days remaining
- [x] **T4.5** Deterministic insight engine (8 analyses, each stating its rule) — PRD §F7
- [x] **T4.6** Insights page UI: cards grouped by severity, no model required

---

## Phase 5 — Agentic Layer

- [x] **T5.1** Ollama HTTP client: `/api/tags`, `/api/ps`, `/api/chat` with NDJSON streaming, tool definitions
- [x] **T5.2** Model router: task → model map with per-task `keep_alive` (PRD §8.1), health check, degradation banner
- [x] **T5.3** Tool registry: closed enum, JSON-schema args, typed validators
- [x] **T5.4** Tools — read: `search_products`, `get_stock`, `list_expiring`, `recent_activity`, `stock_value_by_category`
- [x] **T5.5** Tools — write: `create_product`, `add_batch`, `sell_stock`, `adjust_stock` — all behind confirmation gate
- [x] **T5.6** Deterministic intent parser: verbs, quantities + units, dates, money, negation (PRD §8.2)
- [x] **T5.7** Fuzzy entity resolution against FTS5 + trigram, with `did you mean`
- [x] **T5.8** Tool-loop orchestration: retrieve → plan → execute → narrate, bounded at N steps
- [x] **T5.9** NL→SQL read-only analytics: single-statement validator, `query_only` connection, `LIMIT` injection, table allowlist
- [x] **T5.10** Conversation memory: 6-turn window then summarise
- [x] **T5.11** Agent UI: streaming chat, tool-call cards with args + results, Confirm/Cancel cards, stop button, error states
- [x] **T5.12** Settings: model overrides per task, connection status, degraded-mode toggle

---

## Phase 6 — Hardening & Ship

- [x] **T6.1** Keyboard shortcuts (`⌘K`, `⌘1..5`, `⌘J`, `Esc`, `⌘⏎`)
- [ ] **T6.2** Empty states, loading skeletons, error boundaries, toasts
- [x] **T6.3** `cargo test` for FEFO, ledger invariants, date parsing, intent parsing, SQL guard
- [x] **T6.4** `cargo build` + `npm run build` + `tsc --noEmit` all clean
- [ ] **T6.5** Launch verification against AC-1 … AC-10, screenshots of each page

---

## Acceptance Traceability

| AC | Task |
|---|---|
| AC-1 | T2.2 |
| AC-2 | T2.1 |
| AC-3 | T2.1 |
| AC-4 | T3.1–T3.4 |
| AC-5 | T5.5, T5.11 |
| AC-6 | T5.4, T5.8 |
| AC-7 | T5.2 |
| AC-8 | T5.2 |
| AC-9 | T4.5, T4.6 |
| AC-10 | T6.4 |