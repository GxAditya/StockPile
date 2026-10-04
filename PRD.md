# StockPile — Product Requirements Document

**Version:** 1.0
**Status:** Approved for build
**Date:** 2026-10-04
**Codename:** StockPile — "Agentic Inventory, Stacked Locally"

---

## 1. Executive Summary

StockPile is a desktop-native inventory management application with a **local-first agentic
layer**. It tracks batch-level stock (batch number, expiry date, price, quantity), records an
immutable ledger of every stock addition and sale, and exposes an Ollama-backed agent that answers
natural-language questions *and* mutates inventory through verified tool calls.

Everything runs on the user's machine. No cloud. No API keys. No telemetry.

---

## 2. Problem Statement

Inventory for physical, perishable, batch-tracked goods is usually tracked in spreadsheets that
cannot answer questions. Three concrete failure modes:

1. **Spreadsheets cannot answer "what's about to expire?"** Expiry is one column that is never
   sorted by. Batches quietly become write-offs.
2. **Spreadsheets have no trustworthy history.** Rows get edited. There is no append-only record of
   what came in, what went out, to whom, and at what price. Audits become archaeology.
3. **Entering data is the bottleneck.** A busy operator types the same rows hundreds of times.
   Every keystroke is tax.

StockPile attacks all three: a batch-aware stock model with an append-only ledger (1, 2), and an
agent that turns a spoken/typed sentence into a validated stock mutation (3).

---

## 3. Goals & Non-Goals

### 3.1 Goals

| # | Goal | Success metric |
|---|------|----------------|
| G1 | Batch-level stock display | Every stock unit traceable to (product, batch, expiry, price) |
| G2 | One-keystroke stock in/out | Add or sell from UI **and** from the agent, both auto-updating stock |
| G3 | Append-only history | Ledger shows every inflow and outflow; sales carry buyer name |
| G4 | Agentic NL interface | Natural language answers questions *and* performs mutations |
| G5 | Low-memory agentic features | Agent features idle at **< 400 MB** total app RSS; models unloaded when idle |
| G6 | Offline-first | Full functionality with network disabled |
| G7 | Native-feeling desktop app | Window chrome, keyboard shortcuts, no browser chrome, sub-second startup |

### 3.2 Non-Goals

- No multi-user auth / cloud sync / server mode in v1 (single-operator desktop app).
- No barcode scanning (out of scope; barcode field is stored as plain text for later use).
- No purchase-order workflow / supplier management in v1.
- No image attachments per product in v1.
- No mobile client in v1.
- **No cloud LLM fallback.** If Ollama is down, the deterministic engine still works.

---

## 4. Tech Stack Decision — Tauri v2 (researched, not assumed)

The brief offered a choice between **GPUI** and **Tauri**. This section records the research that
settled it.

### 4.1 Candidate profiles

**GPUI** — Zed Industries' GPU-accelerated Rust UI framework. Hybrid immediate + retained mode, on
`wgpu`/`blade` + `taffy`. Ships on crates.io as `gpui 0.2.2`.

**Tauri v2** — Rust core + OS-native webview (WebKitGTK / WKWebView / WebView2), with an HTML/CSS/TS
frontend.

### 4.2 Evidence gathered (2026-10)

| Signal | GPUI | Tauri v2 |
|---|---|---|
| Upstream momentum | Zed **paused community-facing GPUI development** (Feb 2026) to focus on Zed business priorities. Community fork `gpui-ce` is sparse. | Active, 108k+ stars, formal governance, multi-vendor |
| Release cadence | `gpui 0.2.2` published 2025-10-22; **no release in the ~12 months since**, while `main` diverged heavily (wgpu renderer migration, AccessKit, crate-family split) | Continuous, semver-stable, 2.x line |
| API stability | Pre-1.0. README: *"There will often be breaking changes between versions."* Published crate trails the monorepo | Stable across 2.x |
| **Widget library** | **Ships zero widgets.** A competent developer reported a single text input costing **778 lines**. We need tables, date pickers, modals, tabs, toasts, comboboxes, a chat composer | Mature, best-in-class web ecosystem (TanStack Table, date pickers, chart libs) |
| Accessibility | AccessKit only landed on `main` (May 2026), not in the published crate | Web platform a11y, free |
| Build cost | 704 transitive crates; 6m38s typecheck on a 2-core VM; ~2 GB debug artifacts | ~400 crates, incremental-friendly |
| Footprint | Lowest RAM; no webview process | ~109 MB total incl. all webview helper processes; ~14–57 MB bundle; ~311 ms startup |
| Local env prerequisites | Vulkan device + X11/Wayland compositor | `webkit2gtk-4.1-dev`, `libgtk-3-dev`, `ayatana-appindicator3-dev`, `librsvg2-dev` |

### 4.3 Environment findings

- Ubuntu 24.04, x86_64, 8 cores, 11 GB RAM, 143 GB free disk, `DISPLAY=:0` (X11).
- **All Tauri Linux build dependencies are already present and verified** via `pkg-config`:
  `webkit2gtk-4.1` OK, `gtk+-3.0` OK, plus `ayatana-appindicator3-dev` and `librsvg2-dev`.
  GPUI's Vulkan ICDs are also present, so GPUI would have been buildable.
- **No passwordless `sudo`** is available. This makes missing system packages an unfixable
  blocker. Tauri's prerequisites being pre-satisfied removes that entire class of risk.
- No Rust toolchain was pre-installed; `rustup` stable `1.99.0` was installed as part of setup.
  This cost is shared by both candidates and is therefore not a differentiator.

### 4.4 Decision

> **Choose Tauri v2.**

The deciding factor is **not** RAM or "how native" the stack is — both candidates are fast enough
for this app. The deciding factor is **who pays for the widget layer and the API churn**.

StockPile needs, at minimum: a virtualised data table with sorting/filtering, two date pickers,
three modal dialogs, tabbed navigation, toasts, a streaming chat composer, and charts. On GPUI that
is either several months of writing a design system from scratch, or a strategic dependency on
`gpui-component` — a third-party library tracking a pre-1.0 framework whose upstream just
deprioritised it. Neither is an acceptable risk for the primary product surface.

Tauri additionally gives a Rust backend that is genuinely valuable here: FEFO batch allocation,
ledger integrity, and the Ollama HTTP/streaming client all belong on the trusted side of the
boundary, not in the renderer.

The honest cost of this choice, accepted knowingly: a webview process boundary (~109 MB total RSS,
not 40 MB), three rendering engines across OSes, and serialisation across IPC. For an
inventory app with thousands of rows and a local LLM in the same box, this is a trade we win.

### 4.5 Final stack

**Shell:** Tauri v2 (Rust)
**Frontend:** React 19 + TypeScript 5 + Vite 7
**Styling:** Tailwind CSS v4 (CSS-first `@theme` config)
**Data fetching:** TanStack Query v5 (server state, caching, invalidation)
**Routing:** react-router-dom v7
**Charts:** Recharts (tree-shaken SVG, no canvas runtime)
**Icons:** lucide-react
**Database:** SQLite via `rusqlite` (bundled compile-time SQLite, single-file DB, zero daemon)
**IPC:** Tauri commands (request/response) + events for streaming
**LLM:** Ollama over HTTP (`/api/chat` with tool definitions, NDJSON streaming)
**Build:** Cargo (Rust) + Vite (frontend)

---

## 5. Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  Webview renderer (React 19 + TS)                                   │
│  Dashboard │ Inventory │ Sell │ History │ Agent │ Insights          │
│  TanStack Query cache ──────────────────────────────┐               │
└──────────────────────────────┬──────────────────────┬────────────────┘
                     invoke()  │                      │  listen() events
┌──────────────────────────────▼──────────────────────▼────────────────┐
│  Rust core (Tauri)                                                   │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────────────────────┐  │
│  │ commands   │  │ domain       │  │ agent                       │  │
│  │ (IPC API)  │─▶│ (FEFO, ledger│─▶│ router → intent → tools →   │  │
│  │            │  │  invariants) │  │ nl2sql → narrator           │  │
│  └────────────┘  └──────┬───────┘  └───────────┬──────────────────┘  │
│                        │                      │                     │
│                  ┌─────▼──────┐        ┌──────▼───────┐             │
│                  │ SQLite     │        │ Ollama HTTP  │             │
│                  │ (rusqlite) │        │ + keep_alive │             │
│                  └────────────┘        └──────────────┘             │
└──────────────────────────────────────────────────────────────────────┘
```

### 5.1 Layering rules

1. **The renderer never mutates state.** It calls a command; the command owns the transaction.
2. **Every mutation writes a ledger row in the same transaction as the stock change.** If the
   ledger write fails, the stock change rolls back. The ledger is the source of truth for history;
   batch rows are a materialised view of it.
3. **The agent has no direct SQL or filesystem access.** It may only call whitelisted tools, each of
   which is a thin validated wrapper around a command. Read-only analytics go through a
   separate read-only SQLite connection that is opened with `query_only`.
4. **Writes proposed by the agent require explicit user confirmation** unless they are
   additive-only and unambiguous. See §7.5.

---

## 6. Data Model

### 6.1 `products`

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | |
| `name` | TEXT NOT NULL | Product name, unique (case-insensitive) |
| `sku` | TEXT UNIQUE | Optional stock-keeping unit |
| `barcode` | TEXT | Optional, plain text for v2 scanning |
| `category` | TEXT | Free-form grouping |
| `unit` | TEXT | `pcs`, `kg`, `box`, `litre`, … |
| `description` | TEXT | |
| `reorder_point` | REAL | Default reorder threshold in units |
| `lead_time_days` | INTEGER | Supplier lead time, feeds reorder maths |
| `created_at` / `updated_at` | TEXT | ISO-8601 UTC |

### 6.2 `batches` (the stock unit)

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | |
| `product_id` | INTEGER NOT NULL → `products(id)` ON DELETE CASCADE | |
| `batch_no` | TEXT NOT NULL | Batch / lot number |
| `expiry_date` | TEXT | ISO `YYYY-MM-DD`, nullable for non-perishables |
| `unit_price` | REAL NOT NULL | Purchase price per unit |
| `quantity` | INTEGER NOT NULL | Current on-hand units, `>= 0` |
| `received_at` | TEXT NOT NULL | Inbound date |
| `supplier` | TEXT | |
| `notes` | TEXT | |
| UNIQUE | (`product_id`, `batch_no`) | |

A product's total stock is `SUM(batches.quantity)` for that product. Batch `quantity` is the only
place stock lives.

### 6.3 `ledger` (append-only)

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | |
| `ts` | TEXT NOT NULL | ISO-8601 UTC, the display timestamp |
| `kind` | TEXT NOT NULL | `restock` \| `sale` \| `adjustment` \| `void` |
| `product_id` | INTEGER NOT NULL | |
| `product_name` | TEXT NOT NULL | **Denormalised** so history survives product deletion |
| `batch_id` | INTEGER NULL | NULL for adjustments that span batches |
| `batch_no` | TEXT NULL | Denormalised |
| `expiry_date` | TEXT NULL | Denormalised |
| `qty_delta` | INTEGER NOT NULL | `+n` for inbound, `-n` for outbound |
| `unit_price` | REAL NOT NULL | Price at time of transaction |
| `total_value` | REAL NOT NULL | `qty_delta.abs() * unit_price`, signed |
| `buyer_name` | TEXT NULL | **Populated for `sale` rows** |
| `note` | TEXT NULL | |
| `source` | TEXT NOT NULL | `ui` \| `agent` \| `seed` |
| `actor` | TEXT NULL | e.g. `agent:gemma3:1b` when `source='agent'` |

There are **no** `UPDATE` or `DELETE` statements against `ledger` anywhere in the codebase. A
`correction` is expressed as a paired `adjustment` + `void`.

### 6.4 Derived, not stored

Product-level stock, total inventory value, expiry buckets, and all dashboard KPIs are computed by
SQL at read time. No denormalised counters that can drift. SQLite handles this at inventory scale
without caching.

### 6.5 Indices

```sql
CREATE INDEX idx_batches_product  ON batches(product_id);
CREATE INDEX idx_batches_expiry   ON batches(expiry_date) WHERE quantity > 0;
CREATE INDEX idx_ledger_ts        ON ledger(ts DESC);
CREATE INDEX idx_ledger_product   ON ledger(product_id);
CREATE INDEX idx_ledger_kind      ON ledger(kind);
CREATE INDEX idx_ledger_buyer     ON ledger(buyer_name) WHERE kind = 'sale';
CREATE INDEX idx_products_name    ON products(name COLLATE NOCASE);
```

Plus a **FTS5** virtual table (`products_fts`) mirroring `products.name`, `sku`, `category`, and
`description`, kept in sync by triggers. This powers the agent's product lookup without loading a
model.

---

## 7. Feature Specification

### F1 — Inventory Stock Display

Stock is presented at two levels:

- **Product rows** (primary view): name, category, total on-hand across batches, number of live
  batches, weighted-average unit cost, total stock value, earliest expiry, and a health pill
  (`OK` / `LOW` / `OUT` / `EXPIRING` / `EXPIRED`).
- **Batch drill-down** (per product, expandable inline): batch no, expiry date, unit price,
  quantity received, quantity remaining, received date, supplier. Expiry is colour-coded by day
  bucket.

Stock health thresholds:

| State | Condition |
|---|---|
| `EXPIRED` | `expiry_date < today` and `quantity > 0` |
| `EXPIRING` | `expiry_date <= today + 30d` and `quantity > 0` |
| `OUT` | `total_quantity = 0` |
| `LOW` | `0 < total_quantity <= reorder_point` |
| `OK` | otherwise |

Includes: full-text search, category filter, health filter, sort on any column, and CSV export of the
current filtered view.

### F2 — Add Products to Inventory

A form capturing product identity (name, category, unit, SKU, description) and batch details
(batch no, expiry, unit price, quantity, supplier). If the product name already exists, the form
switches to "add batch to existing product" — chosen from a live typeahead backed by FTS5.

On submit, one transaction writes: the `products` row if new, the `batches` row, and a `restock`
ledger row with `source` and `qty_delta = +n`. Stock updates immediately.

Validation: name required; quantity `>= 1`; price `>= 0`; expiry must be a real date; batch no
unique per product.

### F3 — Sell Products from Inventory

A sale captures product, buyer name, quantity, and an optional per-line price override (defaults to
the batch's unit price). **Stock updates automatically.**

**FEFO allocation.** When a product has multiple live batches, a sale drains
**First Expiry, First Out** — earliest expiry first, skipping expired batches unless the user
explicitly ticks "allow expired". A sale of 10 units across 3 batches produces one ledger row per
batch consumed, so every sold unit keeps its batch provenance.

Blocks: selling more than is on hand (surfaces the shortfall); selling expired stock without the
override; duplicate buyer names within one transaction are merged.

### F4 — History

A single append-only timeline with:

- **Every** restock and sale, newest first, with timestamp, kind badge, product, batch no, expiry,
  quantity delta, unit price, line total, and **buyer name on sale rows**.
- Filters: kind, product, buyer, date range, and free-text search.
- Grouped by day with sticky date headers and per-day subtotals.
- Running inventory balance column.
- CSV export of the filtered slice.

### F5 — Agentic Layer (Ollama)

A persistent chat surface, available from every page, that:

1. **Answers questions** about stock, sales, expiry, and value in natural language, grounded in
   real database rows.
2. **Updates inventory** from natural language — "received 40 units of Amoxicillin B2401 expiring
   2027-03-01 at ₹12.50" creates a batch; "sold 5 Panadol to Mr. Sharma" creates a sale.
3. **Runs multi-step plans** — "list everything expiring this month and tell me what to discount"
   chains `query_expiring` → `get_product` → narrative.

Grounding rule: **the model is never asked to remember stock.** It receives retrieved rows from
SQLite and summarises them. This is the single most important reliability decision in the product,
because the locally available models are small (0.27B–4.5B) and would otherwise hallucinate stock
levels.

Agent response anatomy: streamed text, plus structured **tool-call cards** showing what was invoked,
its arguments, and its result. Mutations show a **Confirm / Cancel** gate before committing.

### F6 — Dashboard

KPI tiles (total stock units, inventory value, distinct products, low-stock count, expiring-in-30d
count, expiring-in-7d count, 30-day revenue, 30-day units sold) plus:

- Stock value by category (donut).
- Inbound vs outbound over the last 14 days (grouped bars).
- Top 5 products by units sold, last 30 days (horizontal bars).
- Expiry horizon buckets (expired / ≤7d / ≤30d / ≤90d / >90d).
- Live "Needs attention" list: expired + expiring-soon batches with days remaining.

### F7 — Insights (non-LLM heuristics)

A page of deterministic, zero-model analyses. Each renders as a card with an explanation of the rule
that produced it, because a heuristic nobody can audit is a heuristic nobody trusts.

1. **Expiry risk** — batches by deadline, with days remaining and a suggested action
   (discount / return to supplier / write off).
2. **Dead stock** — no movement in 90+ days while still holding units. Capital tied up.
3. **Reorder suggestions** — reorder point = `avg_daily_usage_30d × lead_time_days × 1.5 + safety`;
   suggested order qty = `reorder_point + avg_daily_usage_30d × lead_time_days − on_hand`.
4. **Concentration risk** — categories holding > 40% of total value.
5. **Price drift** — batch unit price deviating > 15% from the product's weighted average.
6. **Near-stockout velocity** — days of cover remaining = `on_hand ÷ avg_daily_usage_30d`.
7. **Buyer leaderboard** — top buyers by units and value, with recency.
8. **Sales volatility** — day-over-day units sold z-scores, flagging unusual spikes/drops.

---

## 8. Agentic Design — Low-Memory by Construction

The brief asks for agentic features that are *less RAM consuming*. The dominant RAM cost in a local
agentic app is model residency. Three mechanisms keep this bounded.

### 8.1 Mechanism 1 — Model routing by task difficulty

Ollama keeps a model resident for `keep_alive` seconds after a request. StockPile routes each task
to the smallest model that reliably does it and sets `keep_alive` per call:

| Task | Default model | Size | `keep_alive` |
|---|---|---|---|
| Intent classification, slot extraction, "did you mean" | `gemma3:270m` | 0.29 GB | `5m` |
| Structured JSON tool-arg generation | `qwen2.5-coder:0.5b` | 0.40 GB | `5m` |
| Product/entity fuzzy matching, tag suggestion | `gemma3:1b` | 0.82 GB | `5m` |
| Analytical narration, insight explanation | `qwen3:4b` | 2.50 GB | `0` (unload immediately) |
| Complex multi-step reasoning fallback | `gemma3n:e2b` | 5.62 GB | `0` (on demand only) |

A user asking "how many units of X do I have?" never loads a 4B model. The 5.6 GB model is loaded
only when a genuinely hard question arrives, and it is evicted the moment the answer is produced.

Each is user-overridable in Settings, and the router reports which model answered so behaviour is
observable.

### 8.2 Mechanism 2 — Deterministic-first execution

Most inventory operations do not need a language model at all. A rule-based intent parser runs
**before** any model call and resolves the common cases to a verified tool call:

- Verb lexicon (`add|receive|restock|stock in|got in` → restock; `sell|sold|dispatch|ship` → sale;
  `show|list|what|how many|check` → query) with negation and quantity-unit normalisation
  (`2 boxes`, `3 kg`, `dozen`, `a dozen`, `half a kilo` → unit counts).
- Date parsing for expiry (`2027-03-01`, `1 March 2027`, `next month`, `+90 days`, `6mo`).
- Money parsing (`₹12.50`, `rs 40`, `12.5/-`, `forty rupees`).
- Entity resolution by FTS5 + trigram similarity, with `did you mean` disambiguation.

Zero RAM, zero latency, fully deterministic, and always correct. The model is invoked **only** when
the parser is below confidence or the request is genuinely analytical. On a typical workload this
eliminates the majority of inference calls entirely.

### 8.3 Mechanism 3 — Bounded context and zero embeddings

- Retrieval is `LIMIT`-capped (default 40 rows) and projected to a compact text projection before
  being handed to the model. No row dumps.
- The product-matching index is **FTS5**, not an embedding store. No vector database, no embedding
  model, no similarity index in RAM. A 10k-product catalogue costs 0 resident MB.
- Conversation history is summarised past 6 turns rather than replayed.
- No local vector store means no index rebuild, no cold start, and no per-product embedding cost.

### 8.4 Agent safety envelope

| Guard | Implementation |
|---|---|
| Tool allowlist | Closed enum; unknown tool names are rejected before dispatch |
| Argument schema validation | Every tool has a typed validator; a malformed call never reaches SQL |
| Mutation confirmation | Agent-proposed writes require an explicit user click before commit |
| Read-only analytics | Separate connection opened `PRAGMA query_only = ON` |
| SQL guard | Generated SQL must match exactly one statement, begin with `SELECT`/`WITH`, and pass a table allowlist + `LIMIT` injection |
| Output budget | Tool results capped; narratives capped at 220 tokens |
| Failure isolation | Ollama unreachable → deterministic-only mode with a visible banner; app never blocks |

---

## 9. UX & Visual Design

### 9.1 Design language

- **Dark-first.** A deep neutral slate canvas (`#0B0F14`) with a subtle blue-shifted surface ramp.
  Inventory work is long-session; dark is the default. A light theme is available.
- **Semantic colour only.** Accent = indigo/cyan gradient. Status = emerald (ok), amber (low /
  expiring), rose (out / expired), sky (info). Colour never carries meaning alone — every status
  pill also has a glyph and a text label, so it survives colour-blindness.
- **Density.** 13px base for tables, 14px for body. Row height 40px. This is a data tool, not a
  marketing page.
- **Motion.** 120–180ms `cubic-bezier(0.2, 0, 0, 1)` on hover/press/expand. Streaming text uses a
  1s caret. No bouncing, no parallax, no decorative animation.
- **Typography.** `Inter` for UI, `JetBrains Mono` for batch numbers, SKUs, prices, and IDs — tabular
  numerals everywhere a column of numbers is compared.
- **Icons.** `lucide-react`, 1.75px stroke, consistent 16/20px sizing.
- **No template tells.** No purple-on-white gradient hero, no centred everything, no stock-photo
  illustration, no emoji as UI iconography. Left-aligned content, real density, real hierarchy.

### 9.2 Layout

A persistent left rail (72px collapsed / 224px expanded) with icon+label nav, a top bar carrying
the page title, global search (`⌘K`), and the Ollama status dot. The agent panel is a **right-side
dock, 420px**, toggleable on every page, so the agent is always reachable without leaving context.

### 9.3 Interaction principles

- Every destructive or inventory-mutating action shows a confirmation with the exact diff.
- Empty states are instructional, with a primary action — never a shrug.
- Keyboard: `⌘K` search, `⌘1..5` page nav, `⌘J` agent, `⌘⏎` send, `Esc` close, `⌘⏎` submit form.
- Forms validate inline on blur, never on every keystroke.
- Tables sort and filter without a page reload and preserve state on navigation.
- The agent panel streams tokens; a stop button cancels in-flight inference.

---

## 10. Tech Specs

### 10.1 IPC surface

| Command | Purpose |
|---|---|
| `list_products`, `get_product`, `create_product`, `update_product` | Product CRUD |
| `list_batches`, `add_batch` | Batch read / restock |
| `sell_stock` | FEFO sale, multi-row ledger |
| `adjust_stock` | Manual correction |
| `dashboard_summary` | All KPIs + chart series in one round trip |
| `history_page` | Filtered, paginated ledger |
| `insights_*` | Deterministic analyses |
| `export_csv` | Filtered CSV |
| `agent_chat`, `agent_cancel`, `agent_models`, `agent_set_model` | Agent |
| `search_products` | FTS5 typeahead |
| `seed_demo` | Demo data |

Every command returns `Result<T, AppError>` where `AppError` is a serialisable enum with a stable
`code` and human `message`; the renderer maps codes to localised copy rather than pattern-matching
strings.

### 10.2 Performance targets

- Cold start to interactive window: < 900 ms.
- Product list render (5,000 products): < 120 ms.
- Dashboard summary (10k ledger rows): < 150 ms.
- Agent first token (gemma3:270m path): < 400 ms.
- FEFO sale of 100 units across 40 batches: < 15 ms.
- Idle RSS with agent panel open and models unloaded: < 400 MB.

### 10.3 Data location

`~/.local/share/com.stockpile.app/stockpile.db` (Linux), overridable via `STOCKPILE_DB` env var.
WAL mode, `foreign_keys = ON`, `busy_timeout = 5s`.

---

## 11. Acceptance Criteria

**AC-1** Adding a batch of 100 units at ₹12.50 makes the product's total stock 100 and writes exactly
one `restock` ledger row with `qty_delta = +100`.

**AC-2** Selling 5 units where the earliest-expiring batch holds 3 reduces that batch to 0, draws 2
from the next batch, and writes 2 sale ledger rows — both carrying the buyer name.

**AC-3** Selling more than is on hand is rejected with a message naming the shortfall, and the
database is unchanged.

**AC-4** History shows every transaction newest-first, sale rows show buyer name, filters by buyer
and date work, and CSV export matches the filtered view.

**AC-5** "add 40 amox b2401 expiry 2027-03-01 price 12.5" creates/updates the product and batch, and
the confirmation card names the exact mutation before the commit click.

**AC-6** "how many units of amox do I have" answers from the database with no hallucination, and the
row-level source is visible in the tool-call card.

**AC-7** Killing the Ollama server mid-session leaves the app fully usable; the agent panel shows a
degraded banner and deterministic commands still work.

**AC-8** Idle for 6 minutes with the agent open, then checking `ollama ps`, shows no model resident.

**AC-9** Every insight card states the rule that produced it.

**AC-10** `cargo build` and `npm run build` both succeed with zero errors, and `cargo test` passes.

---

## 12. Risks & Mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Small local model emits malformed tool JSON | **High** | Deterministic parser runs first; JSON extracted defensively; any parse failure degrades to a conversational answer, never a crash or a bad write |
| Model invents stock figures | Medium | Model never authors facts; it narrates retrieved rows. Tool results are the only source |
| Ollama not installed / not running | Medium | Health check on boot; app stays fully usable; banner offers the exact command to start it |
| GPUI's ecosystem risk (avoided) | — | Resolved by choosing Tauri — see §4 |
| Webview RAM higher than hoped | Low | Measured ~109 MB total; within the < 400 MB budget. Route tables through virtualisation |
| SQLite write contention | Low | Single writer connection + `busy_timeout`; inventory scale does not need a server DB |
| Scope creep | Medium | Phased Task.md; each phase independently shippable |

---

## 13. Future Scope (explicitly out of v1)

Barcode scanning via camera; supplier & purchase-order management; multi-location stock; CSV bulk
import with conflict resolution; printable batch labels; scheduled expiry digest; local embedding
search (only if catalogue scale outgrows FTS5); multi-user with a local auth layer; packaged `.deb`
and `.AppImage` distribution.