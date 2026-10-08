# Project status

## Current state

RefactorLens is a working single-binary Rust web app. It serves a plain HTML/JS
UI on `127.0.0.1:7878`, sends pasted code to a chosen LLM provider (Ollama,
Anthropic Claude, any OpenAI-compatible server, or a built-in Demo), parses the
teacher-style JSON reply, resolves each lesson's line ranges, and renders a
side-by-side diff with lessons. `cargo test`, `cargo clippy` and `cargo fmt
--check` pass, and the same checks run in CI.

## Done

- HTTP server, routes and request flow (`src/main.rs`).
- Provider layer for Ollama, Anthropic and OpenAI-compatible servers (`src/llm.rs`).
- Teacher prompt, including the one-shot JSON repair prompt (`src/prompt.rs`).
- JSON extraction, snippet location and aligned diffing, with unit tests (`src/analysis.rs`).
- Built-in demo example and its recorded answer (`src/demo.rs`).
- Environment/flag config (`src/config.rs`).
- Build-free UI: HTML, CSS, JS, self-hosted fonts and highlight.js (`ui/`).
- Nix flake with a `devShells.default`, a `packages.default` and `apps.default`.
- CI workflow running fmt, clippy, tests and `node --check`.
- Project bootstrap docs: `flake.nix`, `shell.nix`, `.envrc`, `README.md`,
  `AGENTS.md`, `CLAUDE.md`, `NEXTSTEPS.md`, `.gitignore`.

## In progress

Nothing is half-finished at the moment.

## Next

- Confirm `nix develop` and `nix build` succeed on a real NixOS machine and note
  the result here. (The bootstrap author could not run Nix in this environment.)
- Add word-level highlighting inside changed diff lines (see `CONTRIBUTING.md`).
- Add follow-up questions about a lesson.
- Stream the summary while the model is still writing.
- Add more languages to the picker in `ui/index.html` and `FILE_EXT` in `ui/app.js`.

## Ideas / later

- Translations of the interface.
- Optional persistence of recent runs (server-side, opt-in).

## Last updated

2026-10-08
