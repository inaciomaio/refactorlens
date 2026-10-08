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
- Practise mode: questions built from the changes a run already produced, with
  no new prompt fields. Progress is kept per concept in `localStorage`.
- Nix flake with a `devShells.default`, a `packages.default` and `apps.default`, verified on NixOS: `nix develop`, `nix build` (including the sandboxed check phase with all tests) and the built binary serving HTTP all work.
- `flake.lock` committed, pinning nixpkgs and flake-utils.
- CI workflow running fmt, clippy, tests and `node --check`.
- Project bootstrap docs: `flake.nix`, `shell.nix`, `.envrc`, `README.md`,
  `AGENTS.md`, `CLAUDE.md`, `NEXTSTEPS.md`, `.gitignore`.

## In progress

- The practise mode (Step 1 of the direction below) is complete and working.
  Step 2, remembering concepts across sessions, is not started: the per-concept
  record is written today but not yet used to pick questions by anything other
  than "missed most".

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

### Step 1: practise, with no new model fields (done)

Lives in `ui/app.js` (the `practise` object and the `buildQuestions`,
`renderQuestion`, `revealAnswer` functions) and `ui/style.css` under
"Practise mode". Reachable from **Practise this** in the result toolbar.

- Questions come entirely from what a reply already contains. The model is not
  asked for anything new, so there is no new way for a reply to fail.
- "Pick the improvement": show one change's `before_lines`, offer its
  `after_lines` plus up to three other changes' `after_lines` as wrong answers.
- "What would you change?": used when a change is the only one, since there is
  nothing to compare against.
- A session is capped at six questions, and concepts missed most are asked
  first.
- Score and per-concept results live in `localStorage`, next to the settings.
  Nothing is sent anywhere.

Useful files to read first: `analysis.rs` (`Change`, `locate`) and `app.js`
(`buildQuestions`).

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
