//! RefactorLens: paste code, get an improved version plus a lesson on every change.
//!
//! The server is deliberately small: one page of static UI baked into the
//! binary, and a JSON API the page calls.

mod analysis;
mod config;
mod demo;
mod llm;
mod prompt;

use std::{sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    analysis::{Change, Dependency, Diff},
    config::Settings,
    llm::{Msg, ProviderConfig, ProviderKind},
    prompt::Options,
};

/// Longest input we accept. Bigger files blow past most models' output limits
/// anyway, because the model has to write the whole file back.
const MAX_CODE_CHARS: usize = 60_000;

struct AppState {
    settings: Settings,
    http: reqwest::Client,
}

type Shared = Arc<AppState>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "refactorlens=info".to_string()),
        )
        .init();

    let mut settings = Settings::from_env();
    if let Err(msg) = settings.apply_args(std::env::args().skip(1)) {
        eprintln!("{msg}");
        std::process::exit(if msg == config::HELP { 0 } else { 2 });
    }

    let state = Arc::new(AppState {
        settings: settings.clone(),
        http: llm::http_client(),
    });

    let api = Router::new()
        .route("/config", get(get_config))
        .route("/ollama/models", get(get_ollama_models))
        .route("/improve", post(improve))
        .layer(middleware::from_fn(guard_api));

    let app = Router::new()
        .route("/", get(|| async { asset("index.html") }))
        .route("/{*path}", get(static_file))
        .nest("/api", api)
        .layer(DefaultBodyLimit::max(512 * 1024))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(&settings.addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "Can't listen on {}: {e}. Try another port with --port.",
                settings.addr
            );
            std::process::exit(1);
        }
    };
    println!(
        "RefactorLens is running. Open http://{} in your browser.",
        settings.addr
    );
    axum::serve(listener, app).await.expect("server error");
}

// ---------------------------------------------------------------------------
// Security guard for the API.
//
// The server holds API keys, so we must stop random websites from using it.
// 1. A custom header forces browsers to do a CORS preflight, which we never
//    approve, so other sites can't POST here.
// 2. The Host check blocks "DNS rebinding", where an attacker's domain is
//    re-pointed at 127.0.0.1.
// ---------------------------------------------------------------------------
async fn guard_api(headers: HeaderMap, req: Request, next: Next) -> Response {
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let name = h.rsplit_once(':').map_or(h, |(n, _)| n);
            matches!(name, "localhost" | "127.0.0.1" | "[::1]")
                || std::env::var("REFACTORLENS_ALLOW_ANY_HOST").is_ok()
        })
        .unwrap_or(false);
    let marker_ok = headers.get("x-refactorlens").is_some();
    if !host_ok || !marker_ok {
        return error(
            StatusCode::FORBIDDEN,
            "Requests must come from the RefactorLens page.",
        );
    }
    next.run(req).await
}

// ---------------------------------------------------------------------------
// Static files, compiled into the binary so there is nothing to install.
// ---------------------------------------------------------------------------
fn asset(path: &str) -> Response {
    let (body, mime): (&'static [u8], &str) = match path {
        "index.html" => (
            include_bytes!("../ui/index.html"),
            "text/html; charset=utf-8",
        ),
        "app.js" => (
            include_bytes!("../ui/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "style.css" => (include_bytes!("../ui/style.css"), "text/css; charset=utf-8"),
        "favicon.svg" => (include_bytes!("../ui/favicon.svg"), "image/svg+xml"),
        "vendor/highlight.min.js" => (
            include_bytes!("../ui/vendor/highlight.min.js"),
            "text/javascript; charset=utf-8",
        ),
        "vendor/fonts/atkinson-400.woff2" => (
            include_bytes!("../ui/vendor/fonts/atkinson-400.woff2"),
            "font/woff2",
        ),
        "vendor/fonts/atkinson-700.woff2" => (
            include_bytes!("../ui/vendor/fonts/atkinson-700.woff2"),
            "font/woff2",
        ),
        "vendor/fonts/jbmono-400.woff2" => (
            include_bytes!("../ui/vendor/fonts/jbmono-400.woff2"),
            "font/woff2",
        ),
        "vendor/fonts/jbmono-600.woff2" => (
            include_bytes!("../ui/vendor/fonts/jbmono-600.woff2"),
            "font/woff2",
        ),
        _ => return (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    let mut res = body.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap());
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    res
}

async fn static_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    asset(&path)
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

/// Tells the page what's configured, without ever sending keys back.
async fn get_config(State(s): State<Shared>) -> Json<serde_json::Value> {
    let st = &s.settings;
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "ollama": { "base_url": st.ollama_url, "model": st.ollama_model },
        "anthropic": { "model": st.anthropic_model, "key_set": !st.anthropic_key.is_empty() },
        "openai": { "base_url": st.openai_base_url, "model": st.openai_model, "key_set": !st.openai_key.is_empty() },
        "demo_code": demo::SAMPLE_CODE,
    }))
}

#[derive(Deserialize)]
struct ModelsQuery {
    base_url: Option<String>,
}

async fn get_ollama_models(
    State(s): State<Shared>,
    axum::extract::Query(q): axum::extract::Query<ModelsQuery>,
) -> Response {
    let base = q
        .base_url
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| s.settings.ollama_url.clone());
    match llm::ollama_models(&s.http, &base).await {
        Ok(models) => Json(json!({ "models": models })).into_response(),
        Err(_) => Json(json!({ "models": [], "unreachable": true })).into_response(),
    }
}

#[derive(Deserialize)]
struct ImproveRequest {
    code: String,
    provider: ProviderConfig,
    #[serde(flatten)]
    options: Options,
}

#[derive(Serialize)]
struct ImproveResponse {
    language: String,
    summary: String,
    original_code: String,
    improved_code: String,
    changes: Vec<Change>,
    dependencies: Vec<Dependency>,
    behavior_changes: Vec<String>,
    diff: Diff,
    model: String,
    elapsed_ms: u128,
    allow_libraries: bool,
}

async fn improve(State(s): State<Shared>, Json(req): Json<ImproveRequest>) -> Response {
    let started = Instant::now();
    let code = analysis::normalize(&req.code);
    if code.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "Paste some code first.");
    }
    if code.chars().count() > MAX_CODE_CHARS {
        return error(
            StatusCode::BAD_REQUEST,
            format!(
                "That's more than {MAX_CODE_CHARS} characters. Try one file or one function at a time."
            ),
        );
    }

    let (raw_reply, model) = if req.provider.kind == ProviderKind::Demo {
        if !demo::is_sample(&code) {
            return error(
                StatusCode::BAD_REQUEST,
                "Demo mode only knows the built-in example. Load it with \"Try the example\", or pick a real model in settings.",
            );
        }
        // A short pause so the loading state is visible, like a real run.
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        (demo::reply(), "demo".to_string())
    } else {
        match ask_model(&s, &req, &code).await {
            Ok(r) => r,
            Err(e) => return error(StatusCode::BAD_GATEWAY, e),
        }
    };

    let parsed = match analysis::extract_json(&raw_reply) {
        Ok(p) => p,
        Err(e) => {
            return error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "The model's answer wasn't usable ({e}). Try again, or try a stronger model."
                ),
            );
        }
    };
    let improved = analysis::normalize(&parsed.improved_code);
    if improved.trim().is_empty() {
        return error(
            StatusCode::BAD_GATEWAY,
            "The model didn't return any improved code. Try again, or try a stronger model.",
        );
    }

    let diff = analysis::diff(&code, &improved);
    let changes = analysis::resolve_changes(&code, &improved, parsed.changes);
    tracing::info!(
        model,
        changes = changes.len(),
        added = diff.added,
        removed = diff.removed,
        "improved code"
    );

    Json(ImproveResponse {
        language: parsed.language.to_lowercase(),
        summary: parsed.summary,
        original_code: code,
        improved_code: improved,
        changes,
        // Respect the toggle even if the model ignored the instruction.
        dependencies: if req.options.allow_libraries {
            parsed.dependencies
        } else {
            Vec::new()
        },
        behavior_changes: parsed.behavior_changes,
        diff,
        model,
        elapsed_ms: started.elapsed().as_millis(),
        allow_libraries: req.options.allow_libraries,
    })
    .into_response()
}

/// Ask the model, and give it one chance to fix a reply that isn't valid JSON.
async fn ask_model(
    s: &AppState,
    req: &ImproveRequest,
    code: &str,
) -> Result<(String, String), String> {
    let system = prompt::system_prompt(&req.options);
    let mut messages = vec![Msg::user(prompt::user_prompt(code, &req.options))];

    for attempt in 0..2 {
        let done = llm::complete(&s.http, &req.provider, &s.settings, &system, &messages)
            .await
            .map_err(|e| e.to_string())?;
        if done.truncated {
            return Err(
                "The model ran out of room before finishing. Try a shorter piece of code."
                    .to_string(),
            );
        }
        match analysis::extract_json(&done.text) {
            Ok(_) => return Ok((done.text, done.model)),
            Err(e) if attempt == 0 => {
                tracing::warn!("reply was not valid JSON, asking for a repair: {e}");
                messages.push(Msg::assistant(done.text));
                messages.push(Msg::user(prompt::repair_prompt(&e)));
            }
            Err(e) => {
                return Err(format!(
                    "The model's answer wasn't usable ({e}). Try again, or try a stronger model."
                ));
            }
        }
    }
    unreachable!("the loop always returns")
}
