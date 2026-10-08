# Contributing to RefactorLens

Thanks for helping. This project is meant to be easy to read and easy to change, so the code itself is a learning resource too.

## Getting set up

```sh
cargo run              # start the app on http://127.0.0.1:7878
cargo test             # unit tests
cargo clippy -- -D warnings
cargo fmt
```

The UI in `ui/` is plain HTML, CSS and JavaScript, compiled into the binary with `include_bytes!`. Change a file, rerun `cargo run`, reload the page.

You don't need a model to work on the interface: choose **Demo** in the model settings and use **Try the example**.

## Good first contributions

- **Prompt quality.** Try the tool on real code in your favourite language. If a lesson is wrong, vague or missing, improve `src/prompt.rs` and describe the before and after in your pull request.
- **More languages in the picker.** Add them to the `<select>` in `ui/index.html` and to `FILE_EXT` in `ui/app.js`.
- **Word-level diff highlighting** inside changed lines.
- **Follow-up questions** about a lesson ("why not use X instead?").
- **Streaming** so the summary appears while the model is still writing.
- **Translations** of the interface.

## Guidelines

- Keep it dependency-light. Ask in an issue before adding a crate or a JavaScript library.
- Keep the UI build-free. No bundlers or frameworks.
- Explain your code with comments where the *why* isn't obvious. Learners will read it.
- Add a test for parsing or diff changes in `src/analysis.rs`.
- Interface text: plain words, sentence case, active voice. Error messages say what happened and what to do.

## Reporting a bad lesson

Open an issue with the input code, the model you used, the settings, and what the lesson got wrong. That is the most useful bug report this project can get.
