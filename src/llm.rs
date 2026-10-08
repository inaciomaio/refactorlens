//! Talking to language models.
//!
//! Every provider boils down to the same idea: send a system prompt plus a
//! list of chat messages, get text back. Only the HTTP shape differs.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::Settings;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Ollama,
    Anthropic,
    Openai,
    /// Canned answer for the built-in example. No model needed.
    Demo,
}

/// What the browser sends. Empty fields fall back to environment settings.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub role: &'static str,
    pub content: String,
}

impl Msg {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant",
            content: content.into(),
        }
    }
}

pub struct Completion {
    pub text: String,
    /// True when the model stopped because it hit its output limit.
    pub truncated: bool,
    pub model: String,
}

pub fn http_client() -> Client {
    Client::builder()
        // Local models on a laptop can take a few minutes for a long file.
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client")
}

fn pick(ui: &str, fallback: &str) -> String {
    let ui = ui.trim();
    if ui.is_empty() {
        fallback.trim().to_string()
    } else {
        ui.to_string()
    }
}

pub async fn complete(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
) -> Result<Completion> {
    match cfg.kind {
        ProviderKind::Ollama => ollama(client, cfg, settings, system, messages).await,
        ProviderKind::Anthropic => anthropic(client, cfg, settings, system, messages).await,
        ProviderKind::Openai => openai(client, cfg, settings, system, messages).await,
        ProviderKind::Demo => bail!("demo mode is handled before calling a model"),
    }
}

/// Like `complete`, but hands each piece of text to `on_delta` as it arrives.
///
/// The deltas are only a preview: the caller still gets the whole reply back so
/// it can parse it as JSON. Streaming is a nicety, so a provider that cannot
/// stream simply falls back to one big delta rather than failing.
pub async fn complete_streaming(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<Completion> {
    match cfg.kind {
        ProviderKind::Ollama => {
            {}
            ollama_stream(client, cfg, settings, system, messages, on_delta).await
        }
        ProviderKind::Anthropic => {
            anthropic_stream(client, cfg, settings, system, messages, on_delta).await
        }
        ProviderKind::Openai => {
            openai_stream(client, cfg, settings, system, messages, on_delta).await
        }
        ProviderKind::Demo => bail!("demo mode is handled before calling a model"),
    }
}

/// Pull one line out of an SSE stream, returning the text after `data: `.
/// Blank lines separate events and carry nothing.
fn sse_data(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim_start)
}

/// Turn an HTTP error into a sentence that tells the user what to do.
async fn explain_error(provider: &str, resp: reqwest::Response, model: &str) -> anyhow::Error {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("error"))
                .and_then(|e| e.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    let hint = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "The API key was rejected. Check the key in settings or your environment."
        }
        StatusCode::NOT_FOUND => {
            "The model or endpoint was not found. Check the model name and base URL."
        }
        StatusCode::TOO_MANY_REQUESTS => "Rate limited. Wait a moment and try again.",
        _ => "",
    };
    anyhow!("{provider} returned {status} for model \"{model}\". {hint} Details: {detail}")
}

async fn ollama(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
) -> Result<Completion> {
    let base = pick(&cfg.base_url, &settings.ollama_url);
    let model = pick(&cfg.model, &settings.ollama_model);
    if model.is_empty() {
        bail!("Choose an Ollama model first. List yours with `ollama list`.");
    }
    let mut msgs = vec![json!({"role": "system", "content": system})];
    msgs.extend(
        messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content})),
    );
    let body = json!({
        "model": model,
        "messages": msgs,
        "stream": false,
        "format": "json",
        // Ollama's default context window is small and silently truncates
        // long prompts. 16k tokens fits the prompt, the code and the reply.
        "options": { "temperature": 0.2, "num_ctx": 16384 }
    });
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        if e.is_connect() {
            anyhow!("Can't reach Ollama at {base}. Is it running? Start it with `ollama serve`.")
        } else {
            anyhow!("Ollama request failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(explain_error("Ollama", resp, &model).await);
    }
    let v: Value = resp
        .json()
        .await
        .context("Ollama sent a reply we couldn't read")?;
    let text = v
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let truncated = v.get("done_reason").and_then(Value::as_str) == Some("length");
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

async fn anthropic(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
) -> Result<Completion> {
    let key = pick(&cfg.api_key, &settings.anthropic_key);
    if key.is_empty() {
        bail!("No Anthropic API key. Set ANTHROPIC_API_KEY or paste a key in settings.");
    }
    let model = pick(&cfg.model, &settings.anthropic_model);
    let base = pick(&cfg.base_url, "https://api.anthropic.com");
    let body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": system,
        "messages": messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect::<Vec<_>>(),
    });
    let resp = client
        .post(format!("{}/v1/messages", base.trim_end_matches('/')))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .context("Anthropic request failed")?;
    if !resp.status().is_success() {
        return Err(explain_error("Anthropic", resp, &model).await);
    }
    let v: Value = resp
        .json()
        .await
        .context("Anthropic sent a reply we couldn't read")?;
    let text = v
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let truncated = v.get("stop_reason").and_then(Value::as_str) == Some("max_tokens");
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

/// Works with OpenAI and any server that copies its API:
/// OpenRouter, LM Studio, llama.cpp server, vLLM, Groq, and many more.
async fn openai(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
) -> Result<Completion> {
    let base = pick(&cfg.base_url, &settings.openai_base_url);
    let model = pick(&cfg.model, &settings.openai_model);
    if model.is_empty() {
        bail!("Enter a model name for your OpenAI-compatible server.");
    }
    let key = pick(&cfg.api_key, &settings.openai_key);
    let mut msgs = vec![json!({"role": "system", "content": system})];
    msgs.extend(
        messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content})),
    );
    let mut req = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .json(&json!({ "model": model, "messages": msgs }));
    // Local servers usually need no key, so only send one if we have it.
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() {
            anyhow!("Can't reach the server at {base}. Check the base URL.")
        } else {
            anyhow!("Request failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(explain_error("The server", resp, &model).await);
    }
    let v: Value = resp
        .json()
        .await
        .context("The server sent a reply we couldn't read")?;
    let choice = v.pointer("/choices/0").cloned().unwrap_or(Value::Null);
    let text = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let truncated = choice.get("finish_reason").and_then(Value::as_str) == Some("length");
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

// ---------------------------------------------------------------------------
// Streaming variants
//
// Each provider streams differently: Ollama sends one JSON object per line,
// Anthropic and OpenAI send Server-Sent Events. All three are reduced to the
// same thing here: call `on_delta` with each new piece of text.
// ---------------------------------------------------------------------------

/// Feed a byte stream to `on_line`, one line at a time, without waiting for the
/// whole body. Reused by all three providers because the framing is similar.
async fn for_each_line(
    resp: reqwest::Response,
    mut on_line: impl FnMut(&str) -> bool,
) -> Result<()> {
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("the streaming reply ended early")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim_end_matches('\r').to_string();
            buf.drain(..=nl);
            // Stop early when the caller has seen all it needs.
            if !on_line(&line) {
                return Ok(());
            }
        }
    }
    // A final line may arrive without a trailing newline.
    let tail = buf.trim_end_matches('\r');
    if !tail.is_empty() {
        on_line(tail);
    }
    Ok(())
}

async fn ollama_stream(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<Completion> {
    let base = pick(&cfg.base_url, &settings.ollama_url);
    let model = pick(&cfg.model, &settings.ollama_model);
    if model.is_empty() {
        bail!("Choose an Ollama model first. List yours with `ollama list`.");
    }
    let mut msgs = vec![json!({"role": "system", "content": system})];
    msgs.extend(
        messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content})),
    );
    let body = json!({
        "model": model,
        "messages": msgs,
        "stream": true,
        "format": "json",
        "options": { "temperature": 0.2, "num_ctx": 16384 }
    });
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        if e.is_connect() {
            anyhow!("Can't reach Ollama at {base}. Is it running? Start it with `ollama serve`.")
        } else {
            anyhow!("Ollama request failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(explain_error("Ollama", resp, &model).await);
    }

    let mut text = String::new();
    let mut truncated = false;
    for_each_line(resp, |line| {
        if line.trim().is_empty() {
            return true;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return true;
        };
        if let Some(piece) = v.pointer("/message/content").and_then(Value::as_str) {
            text.push_str(piece);
            on_delta(piece);
        }
        if v.get("done_reason").and_then(Value::as_str) == Some("length") {
            truncated = true;
        }
        true
    })
    .await?;
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

async fn anthropic_stream(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<Completion> {
    let key = pick(&cfg.api_key, &settings.anthropic_key);
    if key.is_empty() {
        bail!("No Anthropic API key. Set ANTHROPIC_API_KEY or paste a key in settings.");
    }
    let model = pick(&cfg.model, &settings.anthropic_model);
    let base = pick(&cfg.base_url, "https://api.anthropic.com");
    let body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": system,
        "stream": true,
        "messages": messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect::<Vec<_>>(),
    });
    let resp = client
        .post(format!("{}/v1/messages", base.trim_end_matches('/')))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .context("Anthropic request failed")?;
    if !resp.status().is_success() {
        return Err(explain_error("Anthropic", resp, &model).await);
    }

    let mut text = String::new();
    let mut truncated = false;
    let mut mid_error: Option<String> = None;
    for_each_line(resp, |line| {
        let Some(data) = sse_data(line) else {
            return true;
        };
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return true;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                if let Some(piece) = v.pointer("/delta/text").and_then(Value::as_str) {
                    text.push_str(piece);
                    on_delta(piece);
                }
            }
            Some("message_delta") => {
                if v.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("max_tokens") {
                    truncated = true;
                }
            }
            Some("error") => {
                // Stop reading and report it once the loop has returned.
                mid_error = Some(
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Anthropic reported an error mid-stream")
                        .to_string(),
                );
                return false;
            }
            _ => {}
        }
        true
    })
    .await?;
    if let Some(msg) = mid_error {
        bail!("{msg}");
    }
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

async fn openai_stream(
    client: &Client,
    cfg: &ProviderConfig,
    settings: &Settings,
    system: &str,
    messages: &[Msg],
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<Completion> {
    let base = pick(&cfg.base_url, &settings.openai_base_url);
    let model = pick(&cfg.model, &settings.openai_model);
    if model.is_empty() {
        bail!("Enter a model name for your OpenAI-compatible server.");
    }
    let key = pick(&cfg.api_key, &settings.openai_key);
    let mut msgs = vec![json!({"role": "system", "content": system})];
    msgs.extend(
        messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content})),
    );
    let mut req = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .json(&json!({ "model": model, "messages": msgs, "stream": true }));
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() {
            anyhow!("Can't reach the server at {base}. Check the base URL.")
        } else {
            anyhow!("Request failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(explain_error("The server", resp, &model).await);
    }

    let mut text = String::new();
    let mut truncated = false;
    for_each_line(resp, |line| {
        let Some(data) = sse_data(line) else {
            return true;
        };
        if data.trim() == "[DONE]" {
            return false; // stop reading
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return true;
        };
        if let Some(piece) = v
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            text.push_str(piece);
            on_delta(piece);
        }
        if v.pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            == Some("length")
        {
            truncated = true;
        }
        true
    })
    .await?;
    Ok(Completion {
        text,
        truncated,
        model,
    })
}

/// Names of models installed in the local Ollama, for the model picker.
pub async fn ollama_models(client: &Client, base: &str) -> Result<Vec<String>> {
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    let v: Value = client
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v.get("models")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter_map(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}
