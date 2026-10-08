//! Settings read once from environment variables at startup.
//! Anything here can also be overridden per request from the settings panel.

use std::env;

#[derive(Debug, Clone)]
pub struct Settings {
    pub addr: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub anthropic_key: String,
    pub anthropic_model: String,
    pub openai_key: String,
    pub openai_base_url: String,
    pub openai_model: String,
}

fn var(name: &str, default: &str) -> String {
    env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

impl Settings {
    pub fn from_env() -> Self {
        Self {
            addr: var("REFACTORLENS_ADDR", "127.0.0.1:7878"),
            ollama_url: var("OLLAMA_URL", "http://127.0.0.1:11434"),
            ollama_model: var("OLLAMA_MODEL", ""),
            anthropic_key: var("ANTHROPIC_API_KEY", ""),
            anthropic_model: var("ANTHROPIC_MODEL", "claude-sonnet-5-5"),
            openai_key: var("OPENAI_API_KEY", ""),
            openai_base_url: var("OPENAI_BASE_URL", "https://api.openai.com/v1"),
            openai_model: var("OPENAI_MODEL", ""),
        }
    }

    /// Apply `--host` and `--port` command-line flags.
    pub fn apply_args(&mut self, mut args: impl Iterator<Item = String>) -> Result<(), String> {
        let (mut host, mut port) = match self.addr.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => (self.addr.clone(), "7878".to_string()),
        };
        while let Some(a) = args.next() {
            let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
            match a.as_str() {
                "--host" => host = value("--host")?,
                "--port" => port = value("--port")?,
                "-h" | "--help" => return Err(HELP.to_string()),
                other => return Err(format!("Unknown argument: {other}\n\n{HELP}")),
            }
        }
        self.addr = format!("{host}:{port}");
        Ok(())
    }
}

pub const HELP: &str = "refactorlens: learn from your own code, improved by an LLM.

USAGE:
    refactorlens [--host HOST] [--port PORT]

The app listens on 127.0.0.1:7878 by default. Open that address in a browser.

ENVIRONMENT:
    REFACTORLENS_ADDR   address to listen on (default 127.0.0.1:7878)
    OLLAMA_URL          default http://127.0.0.1:11434
    OLLAMA_MODEL        default Ollama model, e.g. qwen2.5-coder:14b
    ANTHROPIC_API_KEY   key for Claude models
    ANTHROPIC_MODEL     default claude-sonnet-5-5
    OPENAI_API_KEY      key for an OpenAI-compatible server
    OPENAI_BASE_URL     default https://api.openai.com/v1
    OPENAI_MODEL        model name for that server";
