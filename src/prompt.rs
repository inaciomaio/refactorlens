//! The prompt is the heart of the tool. It asks the model to act as a teacher,
//! not just a rewriter, and to answer in a strict JSON shape we can parse.

use serde::Deserialize;

/// The fixed set of change categories the UI knows how to colour.
pub const CATEGORIES: &[&str] = &[
    "readability",
    "modern-syntax",
    "performance",
    "safety",
    "error-handling",
    "library",
    "structure",
    "bug-fix",
];

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Beginner,
    #[default]
    Intermediate,
    Advanced,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Options {
    /// Allow third-party libraries in the rewrite.
    #[serde(default)]
    pub allow_libraries: bool,
    #[serde(default)]
    pub level: Level,
    /// Areas to prioritise, e.g. ["readability", "performance"]. Empty = balanced.
    #[serde(default)]
    pub focus: Vec<String>,
    /// Optional language/version target, e.g. "Python 3.12" or "C++20".
    #[serde(default)]
    pub target: String,
    /// Optional language hint. Empty or "auto" = detect.
    #[serde(default)]
    pub language: String,
}

pub fn system_prompt(opts: &Options) -> String {
    let libraries = if opts.allow_libraries {
        "You MAY introduce well-established, actively maintained third-party libraries \
         when they make the code clearly better. Prefer the most widely used option. \
         Never add a library for something the standard library already does well. \
         List every new library in `dependencies` with a one-line install command."
    } else {
        "Use ONLY the language's standard library and built-in features. Do NOT add any \
         third-party dependency. Libraries the original already imports may stay. \
         `dependencies` must be an empty array."
    };

    let level = match opts.level {
        Level::Beginner => {
            "The student is a BEGINNER. Explain in plain words, define every term you use, \
             and keep each explanation to 2-4 short sentences. Avoid jargon unless you define it."
        }
        Level::Intermediate => {
            "The student is INTERMEDIATE. They know the basics. Explain the reasoning and \
             trade-offs in 2-4 sentences. Name the relevant concepts precisely."
        }
        Level::Advanced => {
            "The student is ADVANCED. Be concise and precise. Mention complexity, memory, \
             and edge cases where relevant. 1-3 sentences per explanation."
        }
    };

    let focus = if opts.focus.is_empty() {
        "Balance readability, correctness, safety and performance.".to_string()
    } else {
        format!(
            "Prioritise these areas: {}. Still fix clear bugs you notice.",
            opts.focus.join(", ")
        )
    };

    let target = if opts.target.trim().is_empty() {
        "Target the current stable version of the language.".to_string()
    } else {
        format!(
            "Target exactly: {}. Do not use features newer than that.",
            opts.target.trim()
        )
    };

    let categories = CATEGORIES.join(", ");

    format!(
        r#"You are RefactorLens, a patient programming teacher. A student gives you code.
You rewrite it with modern, idiomatic techniques, then explain every change so the student learns.

Rules:
- Keep the same programming language. {target}
- Preserve behaviour. If you change behaviour (for example to fix a bug), say so in `behavior_changes`.
- Do not change code just for the sake of it. If the code is already good, make few or no changes and say so in the summary.
- {libraries}
- {focus}
- Return the COMPLETE improved program in `improved_code`, never a fragment, and no placeholder comments like "rest unchanged".
- Every item in `changes` must match a real edit in `improved_code`. Group related edits into one change.
- Order `changes` from the top of the file to the bottom.
- `before_snippet` must be 1-6 consecutive lines copied EXACTLY from the original code.
- `after_snippet` must be 1-6 consecutive lines copied EXACTLY from `improved_code`.
  If a change only removes code, use "" for `after_snippet`. If it only adds code, use "" for `before_snippet`.
- `category` must be one of: {categories}.
- `concept` names the one idea worth learning from this change (for example "list comprehension", "RAII", "early return") and explains it in general terms, not just for this code.
- {level}

Reply with ONLY a JSON object. No markdown, no code fences, no text before or after it.
The JSON object has exactly this shape:
{{
  "language": "the language name, lowercase, e.g. python",
  "summary": "2-3 sentences: what the code does and the overall direction of the improvements",
  "improved_code": "the full improved code",
  "changes": [
    {{
      "title": "short title, sentence case, under 8 words",
      "category": "one of the categories",
      "before_snippet": "exact original lines",
      "after_snippet": "exact improved lines",
      "what": "one sentence: what changed",
      "why": "why the new version is better",
      "concept": {{ "name": "the concept", "explanation": "a general explanation of the concept" }}
    }}
  ],
  "dependencies": [
    {{ "name": "library name", "purpose": "what it is used for here", "install": "install command" }}
  ],
  "behavior_changes": ["each way the program now behaves differently, or an empty array"]
}}"#
    )
}

pub fn user_prompt(code: &str, opts: &Options) -> String {
    let hint = match opts.language.trim() {
        "" | "auto" => String::new(),
        lang => format!("The code is written in {lang}.\n"),
    };
    format!("{hint}Improve this code and explain the changes:\n\n<code>\n{code}\n</code>")
}

/// Sent once if the first reply was not valid JSON.
pub fn repair_prompt(error: &str) -> String {
    format!(
        "Your previous reply could not be parsed: {error}. \
         Reply again with ONLY the JSON object described in the instructions. \
         No markdown fences and no text outside the object."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(allow_libraries: bool) -> Options {
        Options {
            allow_libraries,
            level: Level::Beginner,
            focus: vec!["performance".into()],
            target: "Python 3.12".into(),
            language: "python".into(),
        }
    }

    #[test]
    fn library_toggle_changes_the_rules() {
        assert!(system_prompt(&opts(false)).contains("Do NOT add any"));
        assert!(system_prompt(&opts(true)).contains("You MAY introduce"));
    }

    #[test]
    fn options_flow_into_prompt() {
        let p = system_prompt(&opts(false));
        assert!(p.contains("BEGINNER"));
        assert!(p.contains("Python 3.12"));
        assert!(p.contains("performance"));
        assert!(user_prompt("x=1", &opts(false)).starts_with("The code is written in python."));
    }
}
