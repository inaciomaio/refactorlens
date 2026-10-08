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
Write the fields in exactly the order below. The student sees `summary` as you
write it, so put the useful part of it first and keep it to 2-3 sentences.
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

/// A lesson the student asked about, if they tagged one. Kept as plain text so
/// the prompt does not depend on the JSON shape in `analysis`.
pub struct LessonContext {
    pub number: usize,
    pub title: String,
    pub category: String,
    pub what: String,
    pub why: String,
    /// The concept the tool attached, if any.
    pub concept: Option<(String, String)>,
}

/// The system prompt for a follow-up question.
///
/// Different from the main prompt in two ways: the answer is prose, not JSON,
/// and the tool is now a teacher answering a student, so it is told to be
/// honest about what it does not know rather than inventing certainty.
pub fn follow_up_system_prompt(opts: &Options, tagged: bool) -> String {
    let level = match opts.level {
        Level::Beginner => {
            "The student is a BEGINNER. Use plain words, define any term you use, and \
             keep the answer short. Prefer a small example over a long explanation."
        }
        Level::Intermediate => {
            "The student is INTERMEDIATE. Explain the reasoning and the trade-offs, and \
             name the relevant concepts precisely."
        }
        Level::Advanced => {
            "The student is ADVANCED. Be concise and precise. Mention complexity, memory \
             and edge cases where they matter."
        }
    };

    let library_rule = if opts.allow_libraries {
        "You may mention well-known third-party libraries when they are the right answer."
    } else {
        "The refactor used the standard library only. Prefer answers that stay inside it, \
         but you may mention a library if it is genuinely the best choice, and say so."
    };

    let scope = if tagged {
        "The student has tagged one specific change from the refactor. Answer about that \
         change, but use the whole file for context when it helps."
    } else {
        "The student is asking about the refactor as a whole."
    };

    format!(
        r#"You are RefactorLens, a patient programming teacher, answering a student's \
follow-up question about a refactor you just explained.

Rules:
- Answer the question that was asked, in prose. No JSON, no headings unless they help.
- Be honest. If the code has a problem you did not mention, say so. If you are unsure, \
  say what you are unsure about rather than inventing confidence.
- If the student's idea is a good one, say so, even when it differs from the refactor.
- If the answer needs code, show a short snippet and keep it to the point.
- Do not restate the whole refactor. Answer the question.
- {scope}
- {library_rule}
- {level}
- Keep the answer to a few short paragraphs. The student can ask again."#
    )
}

/// The first user message for a follow-up: the refactor, so the model has the
/// context, with the run's summary and every change title as a map.
///
/// The code is sent once per follow-up because the server keeps no session. The
/// history the browser sends is deliberately short for the same reason.
#[allow(clippy::too_many_arguments)]
pub fn follow_up_context(
    language: &str,
    original: &str,
    improved: &str,
    summary: &str,
    change_titles: &[String],
) -> String {
    let titles = change_titles
        .iter()
        .enumerate()
        .map(|(i, t)| format!("  {}. {t}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Here is the refactor we are discussing.\n\nLanguage: {language}\n\n\
         What the refactor did: {summary}\n\n\
         Changes, in order:\n{titles}\n\n\
         The original code:\n<original>\n{original}\n</original>\n\n\
         The improved code:\n<improved>\n{improved}\n</improved>"
    )
}

/// The question itself, with the tagged lesson when there is one.
pub fn follow_up_question(question: &str, lesson: Option<&LessonContext>) -> String {
    match lesson {
        Some(l) => {
            let concept = match &l.concept {
                Some((name, explanation)) => {
                    format!("\nConcept: {name} — {explanation}")
                }
                None => String::new(),
            };
            format!(
                "The student tagged change {} ({}), in the category \"{}\".\n\
                 What it did: {}\n\
                 Why it is better: {}{}\n\n\
                 Their question: {question}",
                l.number, l.title, l.category, l.what, l.why, concept
            )
        }
        None => format!("The student's question: {question}"),
    }
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
    fn prompt_embeds_a_valid_json_skeleton() {
        // The skeleton is written as a raw string inside a format!, where
        // braces must be doubled. This catches the escaping slipping.
        let p = system_prompt(&opts(false));
        let start = p
            .find("{\n  \"language\"")
            .expect("no JSON skeleton in the prompt");
        let end = p[start..].rfind('}').expect("skeleton is not closed") + start + 1;
        let skeleton = &p[start..end];
        serde_json::from_str::<serde_json::Value>(skeleton)
            .expect("the JSON skeleton in the prompt is not valid JSON");
    }

    #[test]
    fn summary_is_asked_for_before_the_rest() {
        // Streaming shows the summary first, so it must come before the code.
        let p = system_prompt(&opts(false));
        let summary = p.find("\"summary\"").expect("no summary field");
        let code = p.find("\"improved_code\"").expect("no improved_code field");
        assert!(
            summary < code,
            "summary should be requested before improved_code"
        );
    }

    #[test]
    fn options_flow_into_prompt() {
        let p = system_prompt(&opts(false));
        assert!(p.contains("BEGINNER"));
        assert!(p.contains("Python 3.12"));
        assert!(p.contains("performance"));
        assert!(user_prompt("x=1", &opts(false)).starts_with("The code is written in python."));
    }

    fn lesson() -> LessonContext {
        LessonContext {
            number: 3,
            title: "Loop over items".into(),
            category: "readability".into(),
            what: "The loop walks the lines directly.".into(),
            why: "No index variable is needed.".into(),
            concept: Some(("Direct iteration".into(), "`for x in thing`".into())),
        }
    }

    #[test]
    fn follow_up_prompt_is_prose_not_json() {
        let p = follow_up_system_prompt(&opts(false), false);
        assert!(p.contains("No JSON"));
        assert!(!p.contains("\"improved_code\""));
    }

    #[test]
    fn follow_up_prompt_changes_with_the_level() {
        let mut o = opts(false);
        o.level = Level::Advanced;
        assert!(follow_up_system_prompt(&o, false).contains("ADVANCED"));
        o.level = Level::Beginner;
        assert!(follow_up_system_prompt(&o, false).contains("BEGINNER"));
    }

    #[test]
    fn tagged_and_general_questions_read_differently() {
        let tagged = follow_up_system_prompt(&opts(false), true);
        let general = follow_up_system_prompt(&opts(false), false);
        assert!(tagged.contains("tagged one specific change"));
        assert!(general.contains("as a whole"));
    }

    #[test]
    fn follow_up_context_includes_both_versions_and_the_titles() {
        let ctx = follow_up_context(
            "python",
            "old code",
            "new code",
            "the summary",
            &["First".into(), "Second".into()],
        );
        assert!(ctx.contains("old code"));
        assert!(ctx.contains("new code"));
        assert!(ctx.contains("the summary"));
        assert!(ctx.contains("1. First"));
        assert!(ctx.contains("2. Second"));
    }

    #[test]
    fn follow_up_question_carries_the_lesson_when_tagged() {
        let q = follow_up_question("Why is that faster?", Some(&lesson()));
        assert!(q.contains("change 3"));
        assert!(q.contains("Loop over items"));
        assert!(q.contains("readability"));
        assert!(q.contains("Direct iteration"));
        assert!(q.contains("Why is that faster?"));
    }

    #[test]
    fn follow_up_question_without_a_lesson_is_just_the_question() {
        let q = follow_up_question("Is this thread safe?", None);
        assert!(q.contains("Is this thread safe?"));
        assert!(!q.contains("tagged change"));
    }
}
