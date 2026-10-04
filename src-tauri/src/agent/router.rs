//! Model routing. The dominant RAM cost in a local agentic app is model residency, so each task
//! is assigned the *smallest* model that reliably does it, with an explicit `keep_alive`.
//! A routine stock question never loads a 4B model, and the 5.6 GB reasoning model is evicted the
//! moment it finishes.

use super::ollama::OllamaModel;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    /// Intent classification and slot extraction.
    Classify,
    /// Structured JSON argument generation.
    Extract,
    /// Product/entity fuzzy matching.
    Resolve,
    /// Analytical narration over retrieved rows.
    Narrate,
    /// Hard multi-step reasoning; only loaded when nothing else suffices.
    Reason,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Classify => "classify",
            TaskKind::Extract => "extract",
            TaskKind::Resolve => "resolve",
            TaskKind::Narrate => "narrate",
            TaskKind::Reason => "reason",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub task: TaskKind,
    pub model: String,
    /// `"0"` means unload immediately after the response.
    pub keep_alive: String,
    pub num_ctx: u32,
    pub fell_back: bool,
    pub reason: Option<String>,
}

/// Preference order per task. First entry that is actually installed wins; each list then
/// degrades to progressively more capable (and heavier) models.
fn preference(kind: TaskKind) -> &'static [&'static str] {
    match kind {
        TaskKind::Classify => &["gemma3:270m", "gemma3:1b", "qwen2.5-coder:0.5b", "llama3.2"],
        TaskKind::Extract => &["qwen2.5-coder:0.5b", "gemma3:270m", "gemma3:1b", "qwen3:4b"],
        TaskKind::Resolve => &["gemma3:1b", "gemma3:270m", "qwen2.5-coder:0.5b", "llama3.2"],
        TaskKind::Narrate => &["qwen3:4b", "phi4-mini", "llama3.2", "nemotron-mini", "gemma3n:e2b"],
        TaskKind::Reason => &["gemma3n:e2b", "qwen3:4b", "nemotron-mini", "llama3.2"],
    }
}

/// Tasks that run inside the tool loop *must* land on a model whose template supports tool
/// calling. Ollama answers a `tools` array with HTTP 400 for anything else, and on this machine
/// that excludes gemma3:1b, gemma3:270m, gemma3n:e2b and phi. The tool-loop tasks therefore
/// carry their own ladder; prose-only tasks keep the cheaper models above.
fn tool_preference(kind: TaskKind) -> Option<&'static [&'static str]> {
    match kind {
        // These two participate in the tool loop, so a non-tool model is a dead end.
        TaskKind::Resolve => Some(&[
            "qwen2.5-coder:0.5b",
            "llama3.2",
            "phi4-mini",
            "nemotron-mini",
            "qwen3:4b",
            "functiongemma",
            "qwen2.5-coder:3b",
        ]),
        TaskKind::Extract => Some(&[
            "qwen2.5-coder:0.5b",
            "llama3.2",
            "phi4-mini",
            "qwen3:4b",
            "functiongemma",
        ]),
        TaskKind::Classify | TaskKind::Narrate | TaskKind::Reason => None,
    }
}

fn keep_alive(kind: TaskKind) -> &'static str {
    match kind {
        // Cheap models stay warm: they answer most turns and reloading costs more than holding.
        TaskKind::Classify | TaskKind::Extract | TaskKind::Resolve => "5m",
        // Expensive models are evicted the instant they finish (AC-8).
        TaskKind::Narrate | TaskKind::Reason => "0",
    }
}

fn num_ctx(kind: TaskKind) -> u32 {
    match kind {
        TaskKind::Classify | TaskKind::Extract => 2048,
        TaskKind::Resolve => 4096,
        TaskKind::Narrate => 8192,
        TaskKind::Reason => 8192,
    }
}

/// Operator overrides, persisted in `settings`. An override naming a model that is not
/// installed is ignored rather than failing the turn.
static OVERRIDES: Lazy<RwLock<Vec<(TaskKind, String)>>> = Lazy::new(|| RwLock::new(Vec::new()));

pub fn set_override(task: TaskKind, model: Option<String>) {
    let mut guard = OVERRIDES.write();
    guard.retain(|(t, _)| *t != task);
    if let Some(m) = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
        guard.push((task, m));
    }
}

pub fn overrides() -> Vec<(String, String)> {
    OVERRIDES
        .read()
        .iter()
        .map(|(t, m)| (t.as_str().to_string(), m.clone()))
        .collect()
}

/// Resolve a task to a concrete installed model.
pub fn route(kind: TaskKind, installed: &[OllamaModel]) -> Route {
    let names: Vec<&str> = installed.iter().map(|m| m.name.as_str()).collect();

    let override_model = OVERRIDES
        .read()
        .iter()
        .find(|(t, _)| *t == kind)
        .map(|(_, m)| m.clone());

    if let Some(m) = override_model {
        let hit = names.iter().any(|n| *n == m);
        return Route {
            task: kind,
            model: m.clone(),
            keep_alive: keep_alive(kind).into(),
            num_ctx: num_ctx(kind),
            fell_back: false,
            reason: (!hit).then(|| "configured override is not installed; using it anyway".into()),
        };
    }

    let tool_ladder = tool_preference(kind);

    // `tool_capable` is an explicit requirement for the tool-loop tasks; unknown capability is
    // treated as capable so a failed probe does not lock the app out of its own agent.
    let acceptable = |name: &str, m: &OllamaModel| -> bool {
        if tool_ladder.is_none() || m.supports_tools() {
            true
        } else {
            let _ = name;
            false
        }
    };

    if let Some(ladder) = tool_ladder {
        for &candidate in ladder {
            let matched = names
                .iter()
                .find(|n| {
                    let fam_match = n.split(':').next() == candidate.split(':').next();
                    (fam_match || **n == candidate)
                        && installed.iter().any(|m| m.name == **n && acceptable(n, m))
                })
                .copied();
            if let Some(m) = matched {
                return Route {
                    task: kind,
                    model: m.to_string(),
                    keep_alive: keep_alive(kind).into(),
                    num_ctx: num_ctx(kind),
                    fell_back: false,
                    reason: None,
                };
            }
        }
    }

    // The generic ladder is only consulted for prose-only tasks. Falling through to it for a
    // tool-loop task would silently reintroduce the exact non-tool model the ladder above is
    // there to avoid, while reporting `fell_back: false`.
    for &candidate in if tool_ladder.is_none() { preference(kind) } else { &[][..] } {
        // Ollama accepts `gemma3:270m` for `gemma3:270m` but a `latest` alias resolves to
        // `gemma3:latest`, so match on the family prefix too.
        if names.iter().any(|n| *n == candidate)
            || names
                .iter()
                .any(|n| n.split(':').next() == candidate.split(':').next())
        {
            return Route {
                task: kind,
                model: candidate.to_string(),
                keep_alive: keep_alive(kind).into(),
                num_ctx: num_ctx(kind),
                fell_back: false,
                reason: None,
            };
        }
    }

    // Nothing preferred is installed. Fall back to the smallest model available so the app
    // still works on an unfamiliar Ollama install.
    let smallest = installed
        .iter()
        .filter(|m| acceptable(&m.name, m))
        .min_by_key(|m| m.size)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "gemma3:270m".into());

    let reason = if tool_ladder.is_some() && smallest == "gemma3:270m" {
        "no installed model supports tool calling; the agent will answer in prose only"
    } else {
        "no preferred model installed; using the smallest available"
    };

    Route {
        task: kind,
        model: smallest,
        keep_alive: keep_alive(kind).into(),
        num_ctx: num_ctx(kind),
        fell_back: true,
        reason: Some(reason.into()),
    }
}

/// True when at least one installed model can be sent tool definitions. The agent uses this to
/// decide whether to offer the tool loop at all rather than discovering it via a 400 mid-turn.
pub fn any_tool_capable(installed: &[OllamaModel]) -> bool {
    installed.iter().any(|m| m.supports_tools())
}

/// Documented default routing, surfaced in Settings so the behaviour is inspectable rather
/// than magic. Does not require Ollama to be running.
pub fn default_table() -> Vec<(String, String, String, String)> {
    vec![
        ("classify".into(), preference(TaskKind::Classify)[0].into(), keep_alive(TaskKind::Classify).into(), "0.29 GB".into()),
        ("extract".into(), tool_preference(TaskKind::Extract).unwrap()[0].into(), keep_alive(TaskKind::Extract).into(), "0.40 GB".into()),
        ("resolve".into(), tool_preference(TaskKind::Resolve).unwrap()[0].into(), keep_alive(TaskKind::Resolve).into(), "0.40 GB".into()),
        ("narrate".into(), preference(TaskKind::Narrate)[0].into(), keep_alive(TaskKind::Narrate).into(), "2.50 GB".into()),
        ("reason".into(), preference(TaskKind::Reason)[0].into(), keep_alive(TaskKind::Reason).into(), "5.62 GB".into()),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, size: u64, caps: Option<&[&str]>) -> OllamaModel {
        OllamaModel {
            name: name.into(),
            size,
            details: None,
            capabilities: caps.map(|c| c.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// The bug this guards: on this machine the default `Resolve` model is `gemma3:1b`, which
    /// Ollama reports as completion-only. Routing the tool loop to it returns HTTP 400 and the
    /// agent answers nothing at all.
    #[test]
    fn tool_loop_never_routes_to_a_completion_only_model() {
        let installed = vec![
            model("gemma3:1b", 820_000_000, Some(&["completion"])),
            model("gemma3:270m", 290_000_000, Some(&["completion"])),
            model("qwen2.5-coder:0.5b", 400_000_000, Some(&["completion", "tools"])),
            model("llama3.2", 2_000_000_000, Some(&["completion", "tools"])),
        ];
        let r = route(TaskKind::Resolve, &installed);
        assert!(
            installed.iter().any(|m| m.name == r.model && m.supports_tools()),
            "Resolve routed to {} which cannot be sent tools",
            r.model
        );
        assert_eq!(r.model, "qwen2.5-coder:0.5b", "should take the cheapest tool-capable model");
        assert!(!r.fell_back);
    }

    /// Same guarantee when only non-tool models are installed: fall back, but say so.
    #[test]
    fn tool_loop_falls_back_and_reports_why() {
        let installed = vec![
            model("gemma3:1b", 820_000_000, Some(&["completion"])),
            model("gemma3:270m", 290_000_000, Some(&["completion"])),
        ];
        let r = route(TaskKind::Resolve, &installed);
        assert!(r.fell_back);
        assert!(
            r.reason.as_deref().unwrap_or_default().contains("tool calling"),
            "reason should explain the tool limitation, got {:?}",
            r.reason
        );
    }

    /// Prose-only tasks still get the cheap models. Narrating a table of rows does not need
    /// tool calling, so gating on capability would push every answer onto a heavier model.
    #[test]
    fn narration_still_prefers_cheap_non_tool_models() {
        let installed = vec![
            model("gemma3:1b", 820_000_000, Some(&["completion"])),
            model("gemma3n:e2b", 5_600_000_000, Some(&["completion"])),
        ];
        let r = route(TaskKind::Narrate, &installed);
        assert_eq!(r.model, "gemma3n:e2b");
        assert_eq!(r.keep_alive, "0", "narrate must still unload immediately");
    }

    /// Unknown capability is treated as capable, so a failed probe cannot lock the app out of
    /// its own agent. The router must never black-hole on a capability we could not read.
    #[test]
    fn unknown_capability_is_treated_as_capable() {
        let installed = vec![model("mystery-model", 500_000_000, None)];
        let r = route(TaskKind::Resolve, &installed);
        assert_eq!(r.model, "mystery-model");
        assert!(any_tool_capable(&installed));
    }

    /// An operator override is still honoured — including naming a non-tool model, which is
    /// their explicit choice, so the request is attempted and any error is theirs to see.
    #[test]
    fn explicit_override_wins_over_capability_gating() {
        set_override(TaskKind::Resolve, Some("gemma3:1b".into()));
        let installed = vec![
            model("gemma3:1b", 820_000_000, Some(&["completion"])),
            model("qwen2.5-coder:0.5b", 400_000_000, Some(&["completion", "tools"])),
        ];
        let r = route(TaskKind::Resolve, &installed);
        assert_eq!(r.model, "gemma3:1b");
        set_override(TaskKind::Resolve, None);
    }
}
