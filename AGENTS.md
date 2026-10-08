# AGENTS.md

RefactorLens is a Rust web app: paste code, and an LLM returns an improved
version plus a short lesson explaining every change. The backend is `axum` and
the frontend is plain HTML/CSS/JS baked into the binary with `include_bytes!`.

## Environment

Always run commands inside `nix develop`. If a tool is missing, add it to
`flake.nix` (`devShells.default.packages`). Never install tools globally.

```sh
nix develop        # enter the shell (or `direnv allow` once)
```

## Commands

| Task        | Command                          |
| ----------- | -------------------------------- |
| Install     | (nothing — the dev shell has everything; deps come from `Cargo.lock`) |
| Run         | `cargo run --release` (serves http://127.0.0.1:7878) |
| Build       | `cargo build --release`          |
| Build (Nix) | `nix build` (runs inside a sandbox; flake only sees git-tracked files) |
| Test        | `cargo test`                     |
| Lint        | `cargo clippy --all-targets -- -D warnings` |
| Format      | `cargo fmt` (check with `cargo fmt --check`) |
| JS check    | `node --check ui/app.js`         |

CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy -- -D
warnings`, `cargo test`, then `node --check ui/app.js`. Match that locally.

## Code conventions

- Rust edition 2024, `rustfmt` defaults, 4-space indent.
- Name things the way the existing code does: `snake_case` functions, types
  and enums in `PascalCase`, modules per concern (`analysis`, `config`, `demo`,
  `llm`, `prompt`).
- Keep the UI build-free: plain HTML/CSS/JS, no bundlers or frameworks.
- Comments explain *why*, not *what*. Learners read this code.
- Add a unit test in `src/analysis.rs` when you touch parsing or diffing.
- Error messages say what happened and what to do next, in plain words.
- Keep it dependency-light; ask before adding a crate or a JS library.

## Rules

- Never commit secrets or API keys. Keys come from environment variables
  (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) or the browser settings panel.
- Do not edit `flake.lock` or `Cargo.lock` by hand; only regenerate them when
  intentionally updating dependencies (`nix flake update`, `cargo update`).
- Do not edit generated/build output: `target/`, `result`, `.direnv/`.
- Do not touch the vendored assets in `ui/vendor/` or their licenses in
  `licenses/` unless the task is specifically about them.

## Before finishing a task

1. Run the tests: `cargo test`.
2. Run lint and format: `cargo clippy --all-targets -- -D warnings` and
   `cargo fmt --check`.
3. Update `NEXTSTEPS.md` to reflect what changed.
