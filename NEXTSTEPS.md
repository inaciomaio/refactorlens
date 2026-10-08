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
  no new prompt fields. Concepts are remembered across sessions in
  `localStorage`, with a small alias table so similar names count as one idea.
- Nix flake with a `devShells.default`, a `packages.default` and `apps.default`, verified on NixOS: `nix develop`, `nix build` (including the sandboxed check phase with all tests) and the built binary serving HTTP all work.
- `flake.lock` committed, pinning nixpkgs and flake-utils.
- CI workflow running fmt, clippy, tests and `node --check`.
- Project bootstrap docs: `flake.nix`, `shell.nix`, `.envrc`, `README.md`,
  `AGENTS.md`, `CLAUDE.md`, `NEXTSTEPS.md`, `.gitignore`.

## In progress

Nothing is half-finished at the moment. The practise mode, Steps 1 and 2 of the
direction below, is complete.

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
- A session is capped at six questions, and concepts that are due are asked
  first.
- Score and per-concept results live in `localStorage`, next to the settings.
  Nothing is sent anywhere.

Useful files to read first: `analysis.rs` (`Change`, `locate`) and `app.js`
(`buildQuestions`).

### Step 2: concepts that persist (done)

Also in `ui/app.js`, alongside Step 1.

- **Stable id.** `CONCEPT_ALIASES` folds the known spellings of an idea onto one
  key, so "guard clause" and "early return" share a record. The display name is
  kept separately, so the wording still reads naturally.
- **Spacing.** Each concept records how many times it was seen, how many were
  missed, and the last session it came up in. A concept that is answered right
  waits longer each time (up to five sessions); one that is missed comes back
  next session. The session leads with due concepts and only tops up to four
  questions with ones already known, so a session stays useful rather than
  repeating what is solid.
- **Plain words.** The end-of-session screen groups concepts into "still
  shaky", "new this session" and "feeling solid". No score, no streak, no
  percentage.

To try it: run the example, practise, then run and practise again. Concepts you
miss should come round first.

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
