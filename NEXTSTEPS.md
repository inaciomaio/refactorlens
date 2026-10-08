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
- Nix flake with a `devShells.default`, a `packages.default` and `apps.default`, verified on NixOS: `nix develop`, `nix build` (including the sandboxed check phase with all tests) and the built binary serving HTTP all work.
- `flake.lock` committed, pinning nixpkgs and flake-utils.
- CI workflow running fmt, clippy, tests and `node --check`.
- Project bootstrap docs: `flake.nix`, `shell.nix`, `.envrc`, `README.md`,
  `AGENTS.md`, `CLAUDE.md`, `NEXTSTEPS.md`, `.gitignore`.

## In progress

Nothing is half-finished at the moment.

## Next

- Add word-level highlighting inside changed diff lines (see `CONTRIBUTING.md`).
- Add follow-up questions about a lesson.
- Stream the summary while the model is still writing.
- Add more languages to the picker in `ui/index.html` and `FILE_EXT` in `ui/app.js`.

## Direction: from refactorer to tutor

The tool is strongest where a chat window is weakest: it can point at a real
line and explain *why*. The next step is to use that position to help people
keep the skill, not just get the code. A learner who reads ten lessons and
remembers none has not been served.

A tutor is a mode you choose, not the default. "Improve my code" stays fast and
single-shot; a **Practise** mode turns one run into a short session. Keeping
them separate matters: people who only want the refactor should not be asked to
play a game first.

The mechanism is already here. The model quotes snippets and Rust finds the
lines (`analysis::locate`), so every change has verified line ranges, a
`category`, a `title` and a `Concept`. A quiz is that same data shown in a
different order.

### Step 1: practise, with no new model fields (small, do first)

Build questions entirely from what a reply already contains:

- Hide `improved_code`. Show one change's `before_lines` and ask what is wrong
  with them. Reveal `what` and `why` as the answer.
- Show `before_snippet` and ask which `after_snippet` is right. Distractors come
  from *other* changes in the same reply, so no extra prompt is needed.
- Score lives in `localStorage`, next to the existing settings. Nothing is sent
  anywhere, and the "nothing to install, keys stay local" promise holds.

This reuses `Change`, `Concept` and `Diff` as they are. It can ship without
touching the prompt, which is the best part: no new failure modes, no worse
`truncated` odds.

### Step 2: concepts that persist (the real feature)

- Add a stable id for a concept. `Concept.name` is a free string today, so the
  model may say "early return" one run and "guard clause" the next. Normalise
  the name and keep a small alias table so the same idea is the same key.
- Keep a per-concept record of what has been missed, and re-ask it later. This
  is what makes it a tutor instead of a quiz.
- Show progress in plain words ("guard clauses: still shaky"), never as a grade.

### Tone

- Ask, do not grade. A question the learner gets wrong is a topic, not a
  failure. No streaks, no scores shouted at the top of the screen.
- The pitch is "keep your eye sharp", not "you are getting worse". The second
  one is a chore, and people do not use chores.

## Ideas / later

- Translations of the interface.
- Optional persistence of recent runs (server-side, opt-in).
- A "what would you change?" prompt before the model answers, so the learner
  commits to a guess first and the reveal teaches more.

## Last updated

2026-10-08
