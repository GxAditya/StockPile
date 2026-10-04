//! Ollama HTTP client. Every request carries an explicit `keep_alive` so model residency is a
//! deliberate budget decision rather than a side effect (PRD §8.1).

use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_HOST: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub details: Option<ModelDetails>,
    /// Populated lazily by `capabilities()`. `/api/tags` does not report these, so it stays
    /// `None` until someone asks.
    #[serde(skip)]
    pub capabilities: Option<Vec<String>>,
}

impl OllamaModel {
    /// Whether this model can be sent tool definitions. Ollama rejects a `tools` array with
    /// HTTP 400 for models whose template has no tool-calling support, and on this machine that
    /// is most of the small models — gemma3:1b, gemma3:270m, gemma3n:e2b and phi are
    /// `["completion"]` only. Treating "unknown" as "yes" is what produced a dead agent.
    pub fn supports_tools(&self) -> bool {
        self.capabilities
            .as_ref()
            .map(|c| c.iter().any(|x| x == "tools"))
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetails {
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedModel {
    pub name: String,
    #[serde(rename = "size_vram")]
    pub size_vram: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Debug, Clone, Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<LoadedModel>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    message: Option<Message>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    #[serde(rename = "function")]
    pub function: ToolCallFn,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallFn {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CapabilitiesResponse {
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub done: bool,
}

#[derive(Clone)]
pub struct Ollama {
    host: String,
    http: reqwest::Client,
}

impl Ollama {
    pub fn new() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
        Self::with_host(host)
    }

    pub fn with_host(host: String) -> Self {
        let host = host.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(180))
            .build()
            .expect("reqwest client");
        Self { host, http }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn health(&self) -> AppResult<()> {
        let resp = self
            .http
            .get(format!("{}/api/tags", self.host))
            .send()
            .await
            .map_err(|e| AppError::agent(format!("cannot reach Ollama at {}: {e}", self.host)))?;
        if !resp.status().is_success() {
            return Err(AppError::agent(format!(
                "Ollama returned HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }

    pub async fn list_models(&self) -> AppResult<Vec<OllamaModel>> {
        let resp = self
            .http
            .get(format!("{}/api/tags", self.host))
            .send()
            .await
            .map_err(|e| AppError::agent(format!("cannot reach Ollama: {e}")))?;
        let tags: TagsResponse = resp
            .json()
            .await
            .map_err(|e| AppError::agent(format!("bad tags response: {e}")))?;
        Ok(tags.models)
    }

    /// Fill in each model's `capabilities` from `/api/show`, which is the only endpoint that
    /// reports tool-calling support. A failure here is not fatal: the caller keeps working with
    /// the optimistic default and the request itself will surface a real problem if there is one.
    pub async fn capabilities(&self, models: &mut [OllamaModel]) {
        for m in models.iter_mut() {
            let Ok(resp) = self
                .http
                .post(format!("{}/api/show", self.host))
                .json(&serde_json::json!({ "model": m.name }))
                .send()
                .await
            else {
                continue;
            };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(body) = resp.json::<CapabilitiesResponse>().await else { continue };
            if let Some(caps) = body.capabilities {
                m.capabilities = Some(caps);
            }
        }
    }

    pub async fn running_models(&self) -> AppResult<Vec<LoadedModel>> {
        let resp = self
            .http
            .get(format!("{}/api/ps", self.host))
            .send()
            .await
            .map_err(|e| AppError::agent(format!("cannot reach Ollama: {e}")))?;
        let ps: PsResponse = resp
            .json()
            .await
            .map_err(|e| AppError::agent(format!("bad ps response: {e}")))?;
        Ok(ps.models)
    }

    /// Evict everything. Used by the "unload models" control and on window close so an idle
    /// app holds no model resident (AC-8).
    pub async fn unload_all(&self) -> AppResult<()> {
        let models = self.running_models().await.unwrap_or_default();
        for m in models {
            // `keep_alive: 0` is Ollama's documented immediate-evict signal.
            let _ = self
                .http
                .post(format!("{}/api/generate", self.host))
                .json(&serde_json::json!({
                    "model": m.name,
                    "keep_alive": 0,
                    "prompt": "",
                    "stream": false
                }))
                .send()
                .await;
        }
        Ok(())
    }

    /// Streaming chat. `keep_alive` accepts Ollama's duration syntax (`"5m"`, `"0"`).
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[Value],
        tools: &[Value],
        keep_alive: &str,
        num_ctx: u32,
    ) -> AppResult<ChatOutcome> {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "keep_alive": keep_alive,
            "options": {
                "temperature": 0.1,
                "num_ctx": num_ctx,
                "num_predict": 400,
            },
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }

        let resp = self
            .http
            .post(format!("{}/api/chat", self.host))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::agent(format!("chat request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp
                .json::<ErrorBody>()
                .await
                .ok()
                .and_then(|b| b.error)
                .unwrap_or_default();
            return Err(AppError::agent(format!(
                "Ollama HTTP {status}{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            )));
        }

        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut done = false;
        let mut stream = resp.bytes_stream();

        while let Some(next) = stream.next().await {
            let bytes = next.map_err(|e| AppError::agent(format!("stream error: {e}")))?;
            for line in String::from_utf8_lossy(&bytes).split('\n') {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let chunk: StreamChunk = match serde_json::from_str(line) {
                    Ok(c) => c,
                    Err(_) => continue, // tolerate partial/garbage lines rather than dropping the turn
                };
                if let Some(e) = chunk.error {
                    return Err(AppError::agent(e));
                }
                if let Some(msg) = chunk.message {
                    if let Some(c) = msg.content {
                        content.push_str(&c);
                    }
                    if let Some(tc) = msg.tool_calls {
                        for call in tc {
                            // Models occasionally emit the args as a JSON *string*; accept both.
                            let normalised = ToolCall {
                                function: ToolCallFn {
                                    name: call.function.name.clone(),
                                    arguments: normalise_args(call.function.arguments),
                                },
                            };
                            tool_calls.push(normalised);
                        }
                    }
                }
                if chunk.done.unwrap_or(false) {
                    done = true;
                }
            }
        }

        Ok(ChatOutcome {
            content,
            tool_calls,
            done,
        })
    }

    /// One-shot completion with no tool affordances. Used for classification and extraction,
    /// where offering tools would only confuse a small model.
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        prompt: &str,
        keep_alive: &str,
        num_predict: u32,
    ) -> AppResult<String> {
        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
            "stream": false,
            "keep_alive": keep_alive,
            "options": {"temperature": 0.0, "num_ctx": 2048, "num_predict": num_predict},
        });

        let resp = self
            .http
            .post(format!("{}/api/chat", self.host))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::agent(format!("complete failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp
                .json::<ErrorBody>()
                .await
                .ok()
                .and_then(|b| b.error)
                .unwrap_or_default();
            return Err(AppError::agent(format!("Ollama HTTP {status}: {detail}")));
        }

        let v: Value = resp
            .json()
            .await
            .map_err(|e| AppError::agent(format!("bad chat response: {e}")))?;
        Ok(v["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

impl Default for Ollama {
    fn default() -> Self {
        Self::new()
    }
}

/// Small models emit `"arguments": "{\"name\":\"x\"}"` often enough that handling it is
/// mandatory rather than defensive. An unparseable string becomes an empty object, which the
/// tool validators then reject cleanly.
fn normalise_args(args: Value) -> Value {
    match args {
        Value::String(s) => serde_json::from_str::<Value>(&s).unwrap_or_else(|_| Value::Object(Default::default())),
        Value::Null => Value::Object(Default::default()),
        v @ Value::Object(_) => v,
        other => serde_json::json!({ "value": other }),
    }
}

/// Extract a JSON object from free-form model output. Handles the three shapes small models
/// actually produce: bare JSON, JSON in a ```json fence, and JSON embedded in prose.
pub fn extract_json_object(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        if v.is_object() {
            return Some(v);
        }
    }
    if let Some(start) = t.find('{') {
        // Scan forward balancing braces, respecting string literals so a brace inside a quoted
        // value does not terminate the object early.
        let bytes = t.as_bytes();
        let mut depth = 0usize;
        let mut in_str = false;
        let mut escaped = false;
        for i in start..bytes.len() {
            let c = bytes[i] as char;
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &t[start..=i];
                        if let Ok(v) = serde_json::from_str::<Value>(slice) {
                            return Some(v);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}