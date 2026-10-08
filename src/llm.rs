//! Talking to language models.
//!
//! Every provider boils down to the same idea: send a system prompt plus a
//! list of chat messages, get text back. Only the HTTP shape differs.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
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
