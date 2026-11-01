//! Agent orchestration: deterministic-first routing, a bounded tool loop, grounded narration,
//! and a hard confirmation gate on every mutation.

pub mod extract;
pub mod intent;
pub mod nl2sql;
pub mod ollama;
pub mod router;
pub mod tools;

use crate::db::Db;
use crate::error::AppResult;
use crate::util;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const MAX_TOOL_ROUNDS: usize = 6;
const MAX_CONTEXT_ROWS: usize = 200;
const MEMORY_TURNS: usize = 6;

/// Everything the renderer needs to draw one agent turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentEvent {
    /// A narrative fragment. Streamed token by token.
    #[serde(rename_all = "camelCase")]
    Text {
        delta: String,
    },
    /// A read tool executed. `rows` is what the model was shown.
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool: String,
        args: Value,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        rows: Option<Vec<Value>>,
    },
    /// A write tool is waiting on the operator. Nothing has changed yet.
    #[serde(rename_all = "camelCase")]
    Confirmation {
        call_id: String,
        tool: String,
        summary: String,
        args: Value,
    },
    #[serde(rename_all = "camelCase")]
    Committed {
        tool: String,
        summary: String,
    },
    #[serde(rename_all = "camelCase")]
    Route {
        model: String,
        task: String,
        keep_alive: String,
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    Done {
        text: String,
        changed: bool,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Notice {
        message: String,
    },
}

/// A pending confirmation held server-side so the renderer only has to send an id back.
/// `call_id` is minted once when the confirmation is proposed and is reused both as the
/// `HashMap` key in `AppState::pending` and as the `call_id` in the `Confirmation` event.
/// Generating two different ids (one for storage, one for the event) makes every confirm
/// fail with "no longer pending", no matter how fast the operator clicks.
#[derive(Debug, Clone)]
pub struct PendingMutation {
    pub call_id: String,
    pub tool: tools::Tool,
    pub args: Value,
    pub summary: String,
}

impl PendingMutation {
    fn new(tool: tools::Tool, args: Value, summary: String) -> Self {
        Self {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool,
            args,
            summary,
        }
    }
}

#[derive(Clone)]
pub struct Agent {
    pub ollama: ollama::Ollama,
    pub cancel: Arc<AtomicBool>,
}

impl Agent {
    pub fn new() -> Self {
        Self {
            ollama: ollama::Ollama::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Handle one user turn. `pending` holds any mutation awaiting confirmation from the UI.
    pub async fn handle(
        &self,
        db: &Db,
        _db_path: &std::path::Path,
        session_id: &str,
        message: &str,
        pending: &mut Vec<PendingMutation>,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        self.cancel.store(false, Ordering::Relaxed);

        // ── Deterministic path (PRD §8.2) ───────────────────────────────────────
        // Runs before any model call. Resolving here keeps the common case at zero RAM.
        let parsed = intent::parse(message);
        let (resolved_id, candidates) = match intent::resolve(db, parsed.product_query.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                events.push(AgentEvent::Error {
                    message: e.to_string(),
                });
                return events;
            }
        };

        match intent::verdict(&parsed, resolved_id, &candidates) {
            intent::Verdict::Ambiguous(c) => {
                events.push(AgentEvent::Text {
                    delta: format!(
                        "I found several products matching that. Which one did you mean?\n{}",
                        c.iter()
                            .map(|n| format!("• {n}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                });
                events.push(AgentEvent::Done {
                    text: String::new(),
                    changed: false,
                });
                return events;
            }
            intent::Verdict::Actionable { missing } => {
                events.extend(self.run_deterministic(db, &parsed, resolved_id, &missing, pending));
                return events;
            }
            intent::Verdict::DeferToModel => {}
        }

        // ── Ollama availability ──────────────────────────────────────────────────
        if self.ollama.health().await.is_err() {
            events.extend(self.degraded(db, &parsed).await);
            return events;
        }

        let mut installed = self.ollama.list_models().await.unwrap_or_default();
        // Ollama's `/api/tags` does not report tool support; `/api/show` does. Without this the
        // tool loop can be handed a model whose template has no tool calling, which fails the
        // whole turn with HTTP 400 instead of answering anything.
        self.ollama.capabilities(&mut installed).await;
        let tool_capable = router::any_tool_capable(&installed);

        // ── Model path ──────────────────────────────────────────────────────────
        // Read tools first. A write tool is only offered when the deterministic parser
        // recognised mutation verbs, which stops small models inventing unsolicited writes.
        let wants_write = matches!(
            parsed.action,
            intent::Action::Restock | intent::Action::Sale | intent::Action::Adjust
        );
        let defs = if wants_write {
            tools::all_tool_defs()
        } else {
            tools::read_tool_defs()
        };

        let history = load_memory(db, session_id);
        let system = system_prompt(wants_write);
        let mut messages: Vec<Value> = vec![json!({"role": "system", "content": system})];
        for m in &history {
            messages.push(json!({"role": m.0, "content": m.1}));
        }
        messages.push(json!({"role": "user", "content": message}));

        let mut changed = false;
        let mut collected = String::new();
        let mut produced_text = false;

        for round in 0..MAX_TOOL_ROUNDS {
            if self.cancelled() {
                events.push(AgentEvent::Notice {
                    message: "Stopped.".into(),
                });
                break;
            }
            let route = router::route(router::TaskKind::Resolve, &installed);
            if round == 0 {
                let path = if !tool_capable {
                    // Be honest about the degraded path rather than labelling a prose-only
                    // answer as if tools ran.
                    "model-no-tools"
                } else if wants_write {
                    "model+tools"
                } else {
                    "model+read-tools"
                };
                events.push(AgentEvent::Route {
                    model: route.model.clone(),
                    task: route.task.as_str().into(),
                    keep_alive: route.keep_alive.clone(),
                    path: path.into(),
                });
            }

            let outcome = self
                .ollama
                .chat_stream(
                    &route.model,
                    &messages,
                    if tool_capable { &defs } else { &[] },
                    &route.keep_alive,
                    route.num_ctx,
                )
                .await;

            let outcome = match outcome {
                Ok(o) => o,
                Err(e) => {
                    events.push(AgentEvent::Error {
                        message: e.to_string(),
                    });
                    break;
                }
            };

            if !outcome.content.trim().is_empty() {
                produced_text = true;
                collected.push_str(&outcome.content);
                events.push(AgentEvent::Text {
                    delta: outcome.content.clone(),
                });
            }

            // A tool call from a model we never offered tools to is not trustworthy — it can only
            // be a hallucination, so drop the loop rather than executing it.
            if outcome.tool_calls.is_empty() || !tool_capable {
                break;
            }

            // Push the assistant turn so the model can see what it asked for.
            messages.push(json!({
                "role": "assistant",
                "content": outcome.content,
                "tool_calls": outcome.tool_calls.iter().map(|c| json!({
                    "function": {"name": c.function.name, "arguments": c.function.arguments}
                })).collect::<Vec<_>>()
            }));

            for call in &outcome.tool_calls {
                if self.cancelled() {
                    break;
                }
                let tool = match tools::Tool::parse(&call.function.name) {
                    Some(t) => t,
                    None => {
                        messages.push(json!({
                            "role": "tool",
                            "content": format!("tool '{}' does not exist", call.function.name)
                        }));
                        continue;
                    }
                };

                if tool.is_write() {
                    match tools::prepare_write(db, call, Some("agent".into())) {
                        Ok(p) => {
                            let pending_mut =
                                PendingMutation::new(p.tool, p.args.clone(), p.summary.clone());
                            let call_id = pending_mut.call_id.clone();
                            let summary = pending_mut.summary.clone();
                            let args = pending_mut.args.clone();
                            pending.push(pending_mut);
                            messages.push(json!({
                                "role": "tool",
                                "content": "Waiting for the operator to confirm. Tell the user exactly what you are about to do."
                            }));
                            events.push(AgentEvent::Confirmation {
                                call_id,
                                tool: tool.name().into(),
                                summary,
                                args,
                            });
                        }
                        Err(e) => messages.push(json!({
                            "role": "tool",
                            "content": format!("rejected: {e}")
                        })),
                    }
                    continue;
                }

                match tools::run_read(db, tool, &call.function.arguments) {
                    Ok(res) => {
                        let projection = tools::project(&res.rows, MAX_CONTEXT_ROWS);
                        events.push(AgentEvent::ToolCall {
                            tool: tool.name().into(),
                            args: call.function.arguments.clone(),
                            summary: res.summary.clone(),
                            rows: Some(res.rows.clone()),
                        });
                        messages.push(json!({
                            "role": "tool",
                            "content": format!("{}\n{}", res.summary, projection)
                        }));
                    }
                    Err(e) => {
                        events.push(AgentEvent::Error {
                            message: e.to_string(),
                        });
                        messages.push(json!({
                            "role": "tool",
                            "content": format!("error: {e}")
                        }));
                    }
                }
            }
        }

        // ── Grounded narration ──────────────────────────────────────────────────
        // If the model answered from tool output, stop. Only ask the bigger model to explain
        // when there is something real to narrate, which keeps RAM down on the common path.
        if collected.trim().is_empty() && !produced_text {
            events.extend(self.narrate(message, pending, &mut changed).await);
        } else {
            changed = !pending.is_empty();
        }

        if !collected.trim().is_empty() {
            save_memory(db, session_id, "user", message);
            save_memory(db, session_id, "assistant", collected.trim());
        }

        events.push(AgentEvent::Done {
            text: collected,
            changed,
        });
        events
    }

    /// Run a deterministically-parsed mutation or query. Fully synchronous — no model involved.
    fn run_deterministic(
        &self,
        db: &Db,
        parsed: &intent::ParsedIntent,
        product_id: Option<i64>,
        missing: &[&'static str],
        pending: &mut Vec<PendingMutation>,
    ) -> Vec<AgentEvent> {
        let mut events = vec![AgentEvent::Route {
            model: "none".into(),
            task: "deterministic".into(),
            keep_alive: "0".into(),
            path: "rules".into(),
        }];

        let unit_note = parsed
            .unit
            .as_ref()
            .map(|u| format!(" (stated in '{u}')"))
            .unwrap_or_default();

        match parsed.action {
            intent::Action::Restock => {
                let pid = match product_id {
                    Some(p) => p,
                    None => {
                        events.push(AgentEvent::Text {
                            delta: format!(
                                "I could not find a product for \"{}\". Which product is this?",
                                parsed.product_query.clone().unwrap_or_default()
                            ),
                        });
                        events.push(AgentEvent::Done { text: String::new(), changed: false });
                        return events;
                    }
                };
                let product_name = match db.read(|c| crate::db::repo::find_product(c, pid)) {
                    Ok(p) => p.name,
                    Err(e) => {
                        events.push(AgentEvent::Error { message: e.to_string() });
                        return events;
                    }
                };
                let qty = parsed.quantity.unwrap_or(0);
                let batch_no = match &parsed.batch_no {
                    Some(b) => b.clone(),
                    None => format!(
                        "AUTO-{}",
                        util::add_days(util::today(), 0).replace('-', "")
                    ),
                };
                let args = json!({
                    "productId": pid,
                    "batchNo": batch_no,
                    "quantity": qty,
                    "unitPrice": parsed.price.unwrap_or(0.0),
                    "expiryDate": parsed.expiry,
                    "actor": "agent:deterministic"
                });
                pending.push(PendingMutation::new(
                    tools::Tool::AddBatch,
                    args.clone(),
                    format!(
                        "Receive {qty} × {product_name} into batch {batch_no} at {:.2}{}{}",
                        parsed.price.unwrap_or(0.0),
                        parsed
                            .expiry
                            .as_ref()
                            .map(|e| format!(" (expires {e})"))
                            .unwrap_or_default(),
                        if missing.contains(&"price") { " — price defaulted to 0.00" } else { "" }
                    ),
                ));
                {
                    let p = pending.last().expect("just pushed");
                    events.push(AgentEvent::Confirmation {
                        call_id: p.call_id.clone(),
                        tool: "add_batch".into(),
                        summary: p.summary.clone(),
                        args,
                    });
                }
                if let Some(m) = missing.iter().find(|m| **m == "price") {
                    events.push(AgentEvent::Notice {
                        message: format!("No price in your message — defaulting batch price to 0.00 ({m})."),
                    });
                }
                let _ = unit_note;
            }

            intent::Action::Sale => {
                let pid = match product_id {
                    Some(p) => p,
                    None => {
                        events.push(AgentEvent::Text {
                            delta: "I could not tell which product to sell.".into(),
                        });
                        events.push(AgentEvent::Done { text: String::new(), changed: false });
                        return events;
                    }
                };
                let product_name = match db.read(|c| crate::db::repo::find_product(c, pid)) {
                    Ok(p) => p.name,
                    Err(e) => {
                        events.push(AgentEvent::Error { message: e.to_string() });
                        return events;
                    }
                };
                let qty = parsed.quantity.unwrap_or(0);
                match db.read(|c| crate::db::repo::find_product(c, pid)) {
                    Ok(_) => {
                        let plan = match crate::domain::plan_allocation(db, pid, qty, false) {
                            Ok(p) => p,
                            Err(e) => {
                                events.push(AgentEvent::Error { message: e.to_string() });
                                return events;
                            }
                        };
                        if plan.shortfall > 0 {
                            let have: i64 = plan.allocations.iter().map(|a| a.quantity).sum();
                            events.push(AgentEvent::Text {
                                delta: format!(
                                    "Only {have} unit(s) of {product_name} are available — I cannot sell {qty}. Nothing was changed."
                                ),
                            });
                            events.push(AgentEvent::Done { text: String::new(), changed: false });
                            return events;
                        }
                        let buyer = parsed.buyer.clone().unwrap_or_default();
                        let batches = plan
                            .allocations
                            .iter()
                            .map(|a| format!("{}×{}", a.quantity, a.batch_no))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let args = json!({
                            "productId": pid,
                            "quantity": qty,
                            "buyerName": buyer,
                            "actor": "agent:deterministic"
                        });
                        pending.push(PendingMutation::new(
                            tools::Tool::SellStock,
                            args.clone(),
                            format!(
                                "Sell {qty} × {product_name} to {buyer} (FEFO batches: {batches})"
                            ),
                        ));
                        {
                            let p = pending.last().expect("just pushed");
                            events.push(AgentEvent::Confirmation {
                                call_id: p.call_id.clone(),
                                tool: "sell_stock".into(),
                                summary: p.summary.clone(),
                                args,
                            });
                        }
                        if parsed.buyer.is_none() {
                            events.push(AgentEvent::Notice {
                                message: "No buyer name in your message — the sale will record an empty buyer. Cancel and include one to keep the history useful.".into(),
                            });
                        }
                    }
                    Err(e) => events.push(AgentEvent::Error { message: e.to_string() }),
                }
            }

            intent::Action::Adjust => {
                let pid = match product_id {
                    Some(p) => p,
                    None => return events,
                };
                let delta = -parsed.quantity.unwrap_or(0);
                let reason = "Adjustment requested in natural language";
                let args = json!({
                    "productId": pid,
                    "quantityDelta": delta,
                    "reason": reason,
                    "actor": "agent:deterministic"
                });
                pending.push(PendingMutation::new(
                    tools::Tool::AdjustStock,
                    args.clone(),
                    format!("Adjust stock by {delta:+} ({reason})"),
                ));
                {
                    let p = pending.last().expect("just pushed");
                    events.push(AgentEvent::Confirmation {
                        call_id: p.call_id.clone(),
                        tool: "adjust_stock".into(),
                        summary: p.summary.clone(),
                        args,
                    });
                }
            }

            intent::Action::QueryStock => match product_id {
                Some(pid) => {
                    let name = db
                        .read(|c| crate::db::repo::find_product(c, pid))
                        .map(|p| p.name)
                        .unwrap_or_default();
                    events.push(Self::emit_read(
                        db,
                        tools::Tool::GetStock,
                        json!({"name": name}),
                    ));
                }
                // No product named — answer from the catalogue totals instead of bouncing to a
                // model that will apologise rather than look anything up.
                None => {
                    events.push(Self::inventory_overview(db));
                }
            },

            intent::Action::QueryExpiring => {
                let within = parsed.within_days.unwrap_or(30);
                events.push(Self::emit_read(
                    db,
                    tools::Tool::ListExpiring,
                    json!({"within_days": within}),
                ));
            }
            intent::Action::QueryValue => {
                events.push(Self::emit_read(
                    db,
                    tools::Tool::StockValueByCategory,
                    json!({}),
                ));
            }
            intent::Action::QueryHistory => {
                events.push(Self::emit_read(
                    db,
                    tools::Tool::RecentActivity,
                    json!({"days": 30, "limit": 25}),
                ));
            }
            intent::Action::QueryBuyers => {
                events.push(Self::emit_read(
                    db,
                    tools::Tool::RecentActivity,
                    json!({"days": 90}),
                ));
            }
            intent::Action::Unknown => {}
        }

        events.push(AgentEvent::Done {
            text: String::new(),
            changed: !pending.is_empty(),
        });
        events
    }

    /// A direct answer to "how many products / units do I have?" — computed here rather than
    /// handed to a model, because it is a single aggregate query with a single correct answer.
    fn inventory_overview(db: &Db) -> AgentEvent {
        let q = |c: &rusqlite::Connection| -> AppResult<(i64, i64, i64, i64, f64)> {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM products),
                        (SELECT COALESCE(SUM(quantity),0) FROM batches WHERE quantity > 0),
                        (SELECT COUNT(*) FROM batches WHERE quantity > 0),
                        (SELECT COUNT(*) FROM batches
                          WHERE quantity > 0 AND expiry_date IS NOT NULL
                            AND expiry_date <= date('now','+30 days')),
                        (SELECT COALESCE(SUM(quantity*unit_price),0)
                         FROM batches WHERE quantity > 0)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        };
        match db.read(|c| q(c)) {
            Ok((products, units, batches, expiring, value)) => AgentEvent::ToolCall {
                tool: "inventory_overview".into(),
                summary: format!(
                    "{} products · {} units across {} live batches · {} expiring within 30 days",
                    products, units, batches, expiring
                ),
                args: json!({}),
                rows: Some(vec![json!({
                    "products": products,
                    "unitsOnHand": units,
                    "liveBatches": batches,
                    "expiringWithin30d": expiring,
                    "stockValue": (value * 100.0).round() / 100.0,
                })]),
            },
            Err(e) => AgentEvent::Error {
                message: e.to_string(),
            },
        }
    }

    fn emit_read(db: &Db, tool: tools::Tool, args: Value) -> AgentEvent {
        match tools::run_read(db, tool, &args) {
            Ok(res) => AgentEvent::ToolCall {
                tool: tool.name().into(),
                summary: res.summary,
                rows: Some(res.rows),
                args,
            },
            Err(e) => AgentEvent::Error {
                message: e.to_string(),
            },
        }
    }

    /// Deterministic reads need real rows, so they go through the same path as model reads.
    async fn degraded(&self, db: &Db, parsed: &intent::ParsedIntent) -> Vec<AgentEvent> {
        let mut events = vec![AgentEvent::Notice {
            message: "Ollama is not reachable, so I can only answer with direct inventory lookups. Start it with `ollama serve` to enable full analysis.".into(),
        }];
        let call = match parsed.action {
            intent::Action::QueryExpiring => Some((
                tools::Tool::ListExpiring,
                json!({"within_days": parsed.within_days.unwrap_or(30)}),
            )),
            intent::Action::QueryValue => Some((tools::Tool::StockValueByCategory, json!({}))),
            intent::Action::QueryHistory | intent::Action::QueryBuyers => {
                Some((tools::Tool::RecentActivity, json!({"days": 30, "limit": 25})))
            }
            intent::Action::QueryStock => parsed
                .product_query
                .clone()
                .map(|n| (tools::Tool::GetStock, json!({"name": n}))),
            _ => None,
        };
        if call.is_none() && parsed.action == intent::Action::QueryStock {
            events.push(Self::inventory_overview(db));
            events.push(AgentEvent::Done {
                text: String::new(),
                changed: false,
            });
            return events;
        }
        match call {
            Some((tool, args)) => match tools::run_read(db, tool, &args) {
                Ok(res) => {
                    events.push(AgentEvent::ToolCall {
                        tool: tool.name().into(),
                        args,
                        summary: res.summary.clone(),
                        rows: Some(res.rows.clone()),
                    });
                }
                Err(e) => events.push(AgentEvent::Error { message: e.to_string() }),
            },
            None => events.push(AgentEvent::Text {
                delta: "I could not map that to a lookup without the model. Try asking about a \
                        specific product, what is expiring, recent activity, or inventory value \
                        by category."
                    .into(),
            }),
        }
        events.push(AgentEvent::Done {
            text: String::new(),
            changed: false,
        });
        events
    }

    /// Ask the reasoning model to explain retrieved rows. Only reached when the tool loop
    /// produced no prose, so the expensive model loads rarely.
    async fn narrate(
        &self,
        question: &str,
        pending: &[PendingMutation],
        changed: &mut bool,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let installed = self.ollama.list_models().await.unwrap_or_default();

        // Local-first: the model may read ALL inventory data. When the tool loop produced
        // no prose, always give NL→SQL one chance against the read-only connection before
        // admitting defeat — not just for keyword-matched "analytics" questions. The model
        // itself can also call `run_readonly_query` directly during the tool loop.
        if pending.is_empty() {
            if let Some((route, sql)) = self.propose_sql(&installed, question).await {
                match nl2sql::run(&crate::db::db_path(), &sql) {
                    Ok((names, rows)) => {
                        let table = nl2sql::render(&names, &rows);
                        events.push(AgentEvent::ToolCall {
                            tool: "run_readonly_query".into(),
                            args: json!({ "sql": nl2sql::validate(&sql).unwrap_or(sql.clone()) }),
                            summary: format!("Read-only query returned {} row(s)", rows.len()),
                            rows: None,
                        });
                        let prompt = format!(
                            "Question: {question}\n\nQuery result:\n{table}\n\n\
                             Answer in 2-4 sentences using only the numbers above. \
                             No preamble. If the data is empty, say so plainly."
                        );
                        let out = self
                            .ollama
                            .complete(
                                &route.model,
                                "You are an inventory analyst. Be precise and terse.",
                                &prompt,
                                &route.keep_alive,
                                260,
                            )
                            .await;
                        match out {
                            Ok(t) if !t.trim().is_empty() => {
                                events.push(AgentEvent::Text { delta: t.clone() });
                                events.push(AgentEvent::Done { text: t, changed: false });
                                return events;
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        events.push(AgentEvent::Notice {
                            message: format!("Could not run that analysis: {e}"),
                        });
                    }
                }
            }
        }

        // Last resort. A canned insight dump is not an answer — it ignores the question entirely
        // and just reprints the dashboard, which reads like the agent pretending it helped. Say
        // what is actually true and stop.
        let mut text = if pending.is_empty() {
            "I could not answer that. Try asking about a specific product, what is expiring, \
             recent activity, or inventory value by category."
                .to_string()
        } else {
            "Confirm the change above to record it.".to_string()
        };
        *changed = !pending.is_empty();
        events.push(AgentEvent::Text {
            delta: text.clone(),
        });
        events.push(AgentEvent::Done {
            text: std::mem::take(&mut text),
            changed: *changed,
        });
        events
    }

    async fn propose_sql(
        &self,
        installed: &[ollama::OllamaModel],
        question: &str,
    ) -> Option<(router::Route, String)> {
        let route = router::route(router::TaskKind::Extract, installed);
        let prompt = format!(
            "Write ONE SQLite query answering this question.\n\nQuestion: {question}\n\nSchema:\n{}\n\n\
             Rules: start with SELECT or WITH; only use these tables; no trailing semicolon; \
             no comments; include a LIMIT. Reply with the query only.",
            nl2sql::schema_prompt()
        );
        let raw = self
            .ollama
            .complete(
                &route.model,
                "You emit SQLite queries and nothing else.",
                &prompt,
                &route.keep_alive,
                220,
            )
            .await
            .ok()?;
        let sql = ollama::extract_json_object(&format!("{{\"q\":{:?}}}", raw.trim()))
            .and_then(|v| v["q"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                raw.trim()
                    .trim_start_matches("```sql")
                    .trim_start_matches("```")
                    .trim_end_matches("```")
                    .trim()
                    .trim_end_matches(';')
                    .to_string()
            });
        nl2sql::validate(&sql).ok()?;
        Some((route, sql))
    }
}

// ── Memory ──────────────────────────────────────────────────────────────────────────────

type MemoryRow = (String, String);

fn load_memory(db: &Db, session_id: &str) -> Vec<MemoryRow> {
    let cutoff = (MEMORY_TURNS * 2) as i64;
    db.read(|c| {
        let mut st = match c.prepare(
            "SELECT role, content FROM agent_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let rows = st.query_map(rusqlite::params![session_id, cutoff], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        });
        let mut v: Vec<MemoryRow> = match rows {
            Ok(rs) => rs.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        };
        v.reverse();
        Ok(v)
    })
    .unwrap_or_default()
}

fn save_memory(db: &Db, session_id: &str, role: &str, content: &str) {
    let trimmed = util::truncate_chars(content.trim(), 1200);
    if trimmed.is_empty() {
        return;
    }
    let _ = db.write(|c| {
        c.execute(
            "INSERT INTO agent_messages (session_id, ts, role, content) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session_id, util::now_ts(), role, trimmed],
        )?;
        Ok(())
    });
}

/// Trim history so it never grows without bound; the window above reads only the tail.
pub fn trim_memory(db: &Db, session_id: &str) {
    let _ = db.write(|c| {
        c.execute(
            "DELETE FROM agent_messages WHERE session_id = ?1 AND id NOT IN (
                 SELECT id FROM agent_messages WHERE session_id = ?1
                 ORDER BY id DESC LIMIT 100)",
            [session_id],
        )?;
        Ok(())
    });
}

pub fn clear_memory(db: &Db, session_id: &str) -> AppResult<()> {
    db.write(|c| {
        c.execute(
            "DELETE FROM agent_messages WHERE session_id = ?1",
            [session_id],
        )?;
        Ok(())
    })
}

// ── Prompts ─────────────────────────────────────────────────────────────────────────────

fn system_prompt(wants_write: bool) -> String {
    let base = "\
You are StockPile's inventory assistant. You run entirely on the operator's machine.

ABSOLUTE RULES
1. Never state a quantity, price, stock level, or date that is not present in a tool result.
   If you have not called a tool, you do not know the answer.
2. Call a tool whenever the answer depends on inventory data. Do not answer from memory.
3. You have FULL read access to all inventory data. Narrow tools (search_products, get_stock,
   list_expiring, recent_activity, stock_value_by_category, low_stock, top_sellers) cover the
   common cases; when they do not answer the question, use run_readonly_query with your own
   SELECT/WITH over products, batches, ledger, usage_daily (plus settings, agent_messages).
   Join and aggregate freely. Always include a LIMIT (max 500 rows).
4. Prefer search_products before get_stock when the product name is a fragment.
5. Dates are YYYY-MM-DD. Quantities are whole units.
";
    if wants_write {
        format!(
            "{base}\
6. You may change inventory, but ONLY what the operator explicitly asked for. \
   Do not bundle extra changes. Do not restock something because it looks low.
7. For a write, state plainly what you are about to do and let the confirmation card do the rest.
8. If a required detail is missing (quantity, batch number, buyer), ask for it instead of guessing.

ANSWER STYLE: two or three sentences. No preamble. Use plain numbers."
        )
    } else {
        format!(
            "{base}\
6. This is a read-only request. Do not attempt to modify anything.

ANSWER STYLE: two or three sentences. Lead with the answer, then one line of supporting detail. \
No preamble, no restating the question, no lists unless asked."
        )
    }
}

/// Default routing table for the Settings page.
pub fn routing_table() -> Vec<(String, String, String, String)> {
    router::default_table()
}