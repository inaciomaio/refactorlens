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
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures_util::stream::unfold;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use tokio::sync::mpsc;

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
        .route("/improve/stream", post(improve_stream))
        .route("/follow-up", post(follow_up))
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
    match build_response(&code, parsed, &model, req.options.allow_libraries, started) {
        Ok(r) => Json(r).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, e),
    }
}

/// Turn a raw model reply into everything the page needs.
///
/// Split out from `improve` so the streaming handler can reuse it: both go from
/// "the model said this" to "here is the diff and the lessons".
fn build_response(
    code: &str,
    parsed: analysis::ModelReply,
    model: &str,
    allow_libraries: bool,
    started: Instant,
) -> Result<ImproveResponse, String> {
    let improved = analysis::normalize(&parsed.improved_code);
    if improved.trim().is_empty() {
        return Err(
            "The model didn't return any improved code. Try again, or try a stronger model."
                .to_string(),
        );
    }

    let diff = analysis::diff(code, &improved);
    let changes = analysis::resolve_changes(code, &improved, parsed.changes);
    tracing::info!(
        model,
        changes = changes.len(),
        added = diff.added,
        removed = diff.removed,
        "improved code"
    );

    Ok(ImproveResponse {
        language: parsed.language.to_lowercase(),
        summary: parsed.summary,
        original_code: code.to_string(),
        improved_code: improved,
        changes,
        // Respect the toggle even if the model ignored the instruction.
        dependencies: if allow_libraries {
            parsed.dependencies
        } else {
            Vec::new()
        },
        behavior_changes: parsed.behavior_changes,
        diff,
        model: model.to_string(),
        elapsed_ms: started.elapsed().as_millis(),
        allow_libraries,
    })
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

// ---------------------------------------------------------------------------
// Streaming: the summary appears while the model is still writing.
//
// Only the summary is streamed. The rest of the reply is JSON that cannot be
// read until it is complete, and the lessons are the point of the tool, so the
// stream exists to show that work is happening, not to replace the final
// result. The last event carries the same payload as POST /api/improve.
// ---------------------------------------------------------------------------
async fn improve_stream(State(s): State<Shared>, Json(req): Json<ImproveRequest>) -> Response {
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

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(8);
    let shared = s.clone();
    tokio::spawn(async move {
        let started = Instant::now();
        let out = run_stream(&shared, req, code, &tx, started).await;
        if let Err(msg) = out {
            let _ = tx.send(Ok(Event::default().event("error").data(msg))).await;
        }
    });

    let stream = unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// Do the work and push events to the channel. Every early return sends its
/// message through `run_stream`'s error, which the caller turns into an event.
async fn run_stream(
    s: &AppState,
    req: ImproveRequest,
    code: String,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    started: Instant,
) -> Result<(), String> {
    // Demo mode has no model to stream from; send the summary in one piece.
    if req.provider.kind == ProviderKind::Demo {
        if !demo::is_sample(&code) {
            return Err(
                "Demo mode only knows the built-in example. Load it with \"Try the example\", or pick a real model in settings."
                    .to_string(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let raw = demo::reply();
        let parsed = analysis::extract_json(&raw)?;
        let _ = tx
            .send(Ok(Event::default()
                .event("summary")
                .data(parsed.summary.clone())))
            .await;
        return finish(
            tx,
            &code,
            parsed,
            "demo",
            req.options.allow_libraries,
            started,
        )
        .await;
    }

    let system = prompt::system_prompt(&req.options);
    let messages = vec![Msg::user(prompt::user_prompt(&code, &req.options))];

    // The delta callback runs on the same task as the provider stream. It sends
    // the whole summary so far each time more text arrives, so the page can
    // simply replace what it shows.
    let mut shown = String::new();
    let mut last_sent = String::new();
    let mut on_delta = |piece: &str| {
        shown.push_str(piece);
        if let Some(text) = analysis::partial_summary(&shown) {
            if text != last_sent {
                last_sent = text.clone();
                // try_send: a full channel means the page is behind, and we would
                // rather drop a preview frame than block the model request.
                let _ = tx.try_send(Ok(Event::default().event("summary").data(text)));
            }
        }
    };

    let done = llm::complete_streaming(
        &s.http,
        &req.provider,
        &s.settings,
        &system,
        &messages,
        &mut on_delta,
    )
    .await
    .map_err(|e| e.to_string());
    let done = done?;

    if done.truncated {
        return Err(
            "The model ran out of room before finishing. Try a shorter piece of code.".to_string(),
        );
    }
    let parsed = analysis::extract_json(&done.text)?;
    finish(
        tx,
        &code,
        parsed,
        &done.model,
        req.options.allow_libraries,
        started,
    )
    .await
}

/// Send the final event: the same JSON shape as a normal run.
async fn finish(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    code: &str,
    parsed: analysis::ModelReply,
    model: &str,
    allow_libraries: bool,
    started: Instant,
) -> Result<(), String> {
    let response = build_response(code, parsed, model, allow_libraries, started)?;
    let body = serde_json::to_string(&response).map_err(|e| e.to_string())?;
    let _ = tx.send(Ok(Event::default().event("done").data(body))).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Follow-up questions.
//
// The student can ask about the whole refactor or about one tagged change. The
// answer is prose, so it streams as it is written rather than arriving as JSON.
//
// The server keeps no session, like the rest of the app: the browser sends the
// refactor and the conversation so far with every question. That costs some
// tokens on a long chat, and in return there is nothing stored and nothing to
// clean up.
// ---------------------------------------------------------------------------

/// One lesson the student tagged, as sent by the browser.
#[derive(Deserialize)]
struct TaggedLesson {
    #[serde(default)]
    number: usize,
    #[serde(default)]
    title: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    what: String,
    #[serde(default)]
    why: String,
    #[serde(default)]
    concept: Option<analysis::Concept>,
}

#[derive(Deserialize)]
struct Turn {
    #[serde(default)]
    question: String,
    #[serde(default)]
    answer: String,
}

#[derive(Deserialize)]
struct FollowUpRequest {
    question: String,
    provider: ProviderConfig,
    #[serde(flatten)]
    options: Options,
    /// The refactor being discussed.
    language: String,
    original_code: String,
    improved_code: String,
    summary: String,
    #[serde(default)]
    change_titles: Vec<String>,
    /// The change the question is about, if the student tagged one.
    #[serde(default)]
    lesson: Option<TaggedLesson>,
    /// Earlier questions and answers, oldest first.
    #[serde(default)]
    history: Vec<Turn>,
}

/// Longest question we accept, so a pasted file cannot stand in for one.
const MAX_QUESTION_CHARS: usize = 2_000;
/// How many earlier turns to replay. Enough to follow a thread, short enough
/// that a long chat does not outgrow the model's context.
const MAX_HISTORY_TURNS: usize = 6;

async fn follow_up(State(s): State<Shared>, Json(req): Json<FollowUpRequest>) -> Response {
    let question = req.question.trim().to_string();
    if question.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Type a question first.");
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return error(
            StatusCode::BAD_REQUEST,
            format!("That's a long question. Keep it under {MAX_QUESTION_CHARS} characters."),
        );
    }

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(8);
    let shared = s.clone();
    tokio::spawn(async move {
        if let Err(msg) = run_follow_up(&shared, req, question, &tx).await {
            let _ = tx.send(Ok(Event::default().event("error").data(msg))).await;
        } else {
            // SSE events with no data line are dropped by clients, so send a
            // word rather than an empty payload: the page uses this to know the
            // answer is complete even if the connection lingers.
            let _ = tx.send(Ok(Event::default().event("done").data("ok"))).await;
        }
    });

    let stream = unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

async fn run_follow_up(
    s: &AppState,
    req: FollowUpRequest,
    question: String,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> Result<(), String> {
    // Demo mode has no model. The built-in answer explains this, so the page
    // can still show the panel instead of a bare error.
    if req.provider.kind == ProviderKind::Demo {
        return Err(
            "Demo mode can't answer questions. Pick a real model in settings, then ask again."
                .to_string(),
        );
    }

    let lesson = req.lesson.as_ref().map(|l| prompt::LessonContext {
        number: l.number,
        title: l.title.clone(),
        category: l.category.clone(),
        what: l.what.clone(),
        why: l.why.clone(),
        concept: l
            .concept
            .as_ref()
            .map(|c| (c.name.clone(), c.explanation.clone())),
    });

    let system = prompt::follow_up_system_prompt(&req.options, lesson.is_some());

    // Turn one: the refactor itself. Later turns: the back-and-forth, so the
    // model can follow "why?" and "what about the other one?".
    let mut messages = vec![Msg::user(prompt::follow_up_context(
        &req.language,
        &req.original_code,
        &req.improved_code,
        &req.summary,
        &req.change_titles,
    ))];
    let start = req.history.len().saturating_sub(MAX_HISTORY_TURNS);
    for turn in &req.history[start..] {
        if !turn.question.trim().is_empty() {
            messages.push(Msg::user(turn.question.clone()));
        }
        if !turn.answer.trim().is_empty() {
            messages.push(Msg::assistant(turn.answer.clone()));
        }
    }
    messages.push(Msg::user(prompt::follow_up_question(
        &question,
        lesson.as_ref(),
    )));

    let mut on_delta = |piece: &str| {
        // try_send: if the page is behind, drop a frame rather than block the
        // model. The text is cumulative, so a dropped frame loses nothing.
        let _ = tx.try_send(Ok(Event::default().event("delta").data(piece)));
    };
    let done = llm::complete_streaming(
        &s.http,
        &req.provider,
        &s.settings,
        &system,
        &messages,
        &mut on_delta,
    )
    .await
    .map_err(|e| e.to_string())?;

    if done.truncated {
        return Err(
            "The model ran out of room while answering. Ask a shorter question.".to_string(),
        );
    }
    if done.text.trim().is_empty() {
        return Err("The model did not answer. Try again, or try a stronger model.".to_string());
    }
    Ok(())
}
