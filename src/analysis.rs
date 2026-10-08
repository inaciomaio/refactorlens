//! Turning a raw LLM reply into something the UI can show.
//!
//! Three jobs live here:
//! 1. Pull a JSON object out of a model reply (models love to add prose or code fences).
//! 2. Find where each change's snippets sit in the original and improved code,
//!    so the UI can link a lesson note to real line numbers.
//! 3. Build an aligned, side-by-side line diff.

use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp, TextDiff};

/// What we ask the model to return. Every field has a default so a slightly
/// sloppy reply still parses.
#[derive(Debug, Deserialize)]
pub struct ModelReply {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub improved_code: String,
    #[serde(default)]
    pub changes: Vec<ModelChange>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub behavior_changes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelChange {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub before_snippet: String,
    #[serde(default)]
    pub after_snippet: String,
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub concept: Option<Concept>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Concept {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub install: String,
}

/// One explained change, with line ranges resolved (1-based, inclusive).
#[derive(Debug, Serialize)]
pub struct Change {
    pub id: usize,
    pub title: String,
    pub category: String,
    pub what: String,
    pub why: String,
    pub concept: Option<Concept>,
    pub before_lines: Option<(usize, usize)>,
    pub after_lines: Option<(usize, usize)>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Equal,
    Removed,
    Added,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Side {
    pub no: usize,
    pub text: String,
    pub kind: LineKind,
}

/// One row of the split view. A missing side is a blank filler cell.
#[derive(Debug, Serialize, PartialEq)]
pub struct Row {
    pub left: Option<Side>,
    pub right: Option<Side>,
}

#[derive(Debug, Serialize)]
pub struct Diff {
    pub rows: Vec<Row>,
    pub added: usize,
    pub removed: usize,
}

/// Normalise line endings and strip a single trailing newline difference,
/// so "a\r\nb" and "a\nb\n" diff as equal.
pub fn normalize(code: &str) -> String {
    let s = code.replace("\r\n", "\n");
    s.trim_end_matches('\n').to_string()
}

/// Find the first `{ ... }` JSON object in a reply and parse it.
///
/// Handles: a bare object, an object wrapped in ```json fences,
/// and an object with chatter before or after it.
pub fn extract_json(reply: &str) -> Result<ModelReply, String> {
    let trimmed = reply.trim();
    if let Ok(parsed) = serde_json::from_str::<ModelReply>(trimmed) {
        return Ok(parsed);
    }
    // Walk the text and find the first balanced object, respecting strings.
    let bytes = trimmed.as_bytes();
    let start = trimmed
        .find('{')
        .ok_or_else(|| "the reply contained no JSON object".to_string())?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &trimmed[start..=i];
                    return serde_json::from_str::<ModelReply>(candidate)
                        .map_err(|e| format!("the JSON was malformed: {e}"));
                }
            }
            _ => {}
        }
    }
    Err("the JSON object was never closed (the reply may have been cut off)".to_string())
}

/// Pull the text of the `summary` field out of a reply that is still arriving.
///
/// While the model writes, its reply is not valid JSON yet, so this reads the
/// characters after `"summary": "` and returns them, stopping at the closing
/// quote. It tolerates a missing closing quote, because that is the normal
/// case while the summary is still being written.
///
/// Returns None when the summary has not started yet.
pub fn partial_summary(reply_so_far: &str) -> Option<String> {
    let key = reply_so_far.find("\"summary\"")?;
    let after_key = &reply_so_far[key + "\"summary\"".len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;

    // Read to the closing quote, honouring backslash escapes, and unstuff them
    // so the preview matches what the parsed JSON will say.
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('u') => {
                    // \uXXXX: copy the escape through rather than guessing.
                    out.push('\\');
                    out.push('u');
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            out.push(h);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    Some(out)
}

/// Locate a snippet inside some code. Returns 1-based inclusive line numbers.
///
/// Matching ignores indentation and blank lines, because models often
/// re-indent snippets. We first try exact (trimmed) line equality; if that
/// fails we accept lines that *contain* the snippet line, which copes with
/// snippets that were shortened.
pub fn locate(code: &str, snippet: &str) -> Option<(usize, usize)> {
    let wanted: Vec<&str> = snippet
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let lines: Vec<&str> = code.lines().collect();

    let try_match = |loose: bool| -> Option<(usize, usize)> {
        let same = |have: &str, want: &str| {
            let have = have.trim();
            if loose {
                // Very short lines like "}" would match everywhere; demand equality for them.
                want.len() >= 4 && have.contains(want)
            } else {
                have == want
            }
        };
        for start in 0..lines.len() {
            if !same(lines[start], wanted[0]) {
                continue;
            }
            let mut idx = start;
            let mut matched = 1;
            let mut end = start;
            while matched < wanted.len() {
                idx += 1;
                if idx >= lines.len() {
                    break;
                }
                if lines[idx].trim().is_empty() {
                    continue;
                }
                if same(lines[idx], wanted[matched]) || lines[idx].trim() == wanted[matched] {
                    matched += 1;
                    end = idx;
                } else {
                    break;
                }
            }
            if matched == wanted.len() {
                return Some((start + 1, end + 1));
            }
        }
        None
    };

    try_match(false).or_else(|| try_match(true))
}

/// Build an aligned split diff. Replaced blocks are paired line by line so
/// "before" and "after" sit next to each other.
pub fn diff(before: &str, after: &str) -> Diff {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let td = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_slices(&old, &new);

    let side = |no: usize, text: &str, kind: LineKind| Side {
        no: no + 1,
        text: text.to_string(),
        kind,
    };

    let mut rows = Vec::new();
    let (mut added, mut removed) = (0, 0);
    for op in td.ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for k in 0..len {
                    rows.push(Row {
                        left: Some(side(old_index + k, old[old_index + k], LineKind::Equal)),
                        right: Some(side(new_index + k, new[new_index + k], LineKind::Equal)),
                    });
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                removed += old_len;
                for k in 0..old_len {
                    rows.push(Row {
                        left: Some(side(old_index + k, old[old_index + k], LineKind::Removed)),
                        right: None,
                    });
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                added += new_len;
                for k in 0..new_len {
                    rows.push(Row {
                        left: None,
                        right: Some(side(new_index + k, new[new_index + k], LineKind::Added)),
                    });
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                removed += old_len;
                added += new_len;
                for k in 0..old_len.max(new_len) {
                    rows.push(Row {
                        left: (k < old_len)
                            .then(|| side(old_index + k, old[old_index + k], LineKind::Removed)),
                        right: (k < new_len)
                            .then(|| side(new_index + k, new[new_index + k], LineKind::Added)),
                    });
                }
            }
        }
    }
    Diff {
        rows,
        added,
        removed,
    }
}

/// Resolve every model change into a UI-ready change with line ranges.
pub fn resolve_changes(original: &str, improved: &str, changes: Vec<ModelChange>) -> Vec<Change> {
    changes
        .into_iter()
        .enumerate()
        .map(|(i, c)| Change {
            id: i + 1,
            before_lines: locate(original, &c.before_snippet),
            after_lines: locate(improved, &c.after_snippet),
            title: c.title,
            category: normalize_category(&c.category),
            what: c.what,
            why: c.why,
            concept: c.concept.filter(|k| !k.name.trim().is_empty()),
        })
        .collect()
}

/// Map whatever the model wrote onto our fixed set of categories.
fn normalize_category(raw: &str) -> String {
    let r = raw.trim().to_lowercase().replace([' ', '_'], "-");
    if crate::prompt::CATEGORIES.contains(&r.as_str()) {
        return r;
    }
    let pick = if r.contains("bug") {
        "bug-fix"
    } else if r.contains("perf") || r.contains("speed") || r.contains("efficien") {
        "performance"
    } else if r.contains("error") || r.contains("exception") {
        "error-handling"
    } else if r.contains("safe") || r.contains("secur") || r.contains("type") {
        "safety"
    } else if r.contains("librar") || r.contains("depend") || r.contains("package") {
        "library"
    } else if r.contains("struct") || r.contains("organ") || r.contains("design") {
        "structure"
    } else if r.contains("modern") || r.contains("idiom") || r.contains("syntax") {
        "modern-syntax"
    } else {
        "readability"
    };
    pick.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_reply() {
        let reply = "Sure! Here you go:\n```json\n{\"summary\": \"ok {braces} in string\", \"improved_code\": \"x = 1\"}\n```\nHope it helps.";
        let parsed = extract_json(reply).unwrap();
        assert_eq!(parsed.summary, "ok {braces} in string");
        assert_eq!(parsed.improved_code, "x = 1");
    }

    #[test]
    fn reports_truncated_json() {
        let err = extract_json("{\"summary\": \"cut").unwrap_err();
        assert!(err.contains("never closed"));
    }

    #[test]
    fn locates_snippet_ignoring_indentation_and_blank_lines() {
        let code = "def f():\n    a = 1\n\n    return a\n";
        assert_eq!(locate(code, "a = 1\nreturn a"), Some((2, 4)));
        assert_eq!(locate(code, "nope"), None);
    }

    #[test]
    fn loose_match_skips_short_lines() {
        let code = "if x {\n}\nlet total = items.iter().sum();\n";
        assert_eq!(locate(code, "items.iter().sum()"), Some((3, 3)));
        assert_eq!(locate(code, "x"), None);
    }

    #[test]
    fn diff_pairs_replacements() {
        let d = diff("a\nb\nc", "a\nB\nc\nd");
        assert_eq!(d.added, 2);
        assert_eq!(d.removed, 1);
        assert_eq!(d.rows.len(), 4);
        let r = &d.rows[1];
        assert_eq!(r.left.as_ref().unwrap().text, "b");
        assert_eq!(r.right.as_ref().unwrap().text, "B");
    }

    #[test]
    fn categories_are_normalized() {
        assert_eq!(normalize_category("Performance"), "performance");
        assert_eq!(normalize_category("Modern Python idiom"), "modern-syntax");
        assert_eq!(normalize_category("??"), "readability");
    }

    #[test]
    fn partial_summary_reads_a_finished_field() {
        let reply = r#"{"language": "python", "summary": "A short lesson.", "improved_code": "x"}"#;
        assert_eq!(partial_summary(reply).as_deref(), Some("A short lesson."));
    }

    #[test]
    fn partial_summary_tolerates_an_unfinished_field() {
        // This is the normal case while the model is still writing.
        let reply = r#"{"language": "python", "summary": "A short less"#;
        assert_eq!(partial_summary(reply).as_deref(), Some("A short less"));
    }

    #[test]
    fn partial_summary_is_none_before_the_field() {
        assert_eq!(partial_summary(r#"{"language": "py"#), None);
    }

    #[test]
    fn partial_summary_unescapes_and_stops_at_the_close_quote() {
        let reply = r#"{"summary": "line one\nline \"two\"", "improved_code": "nope"}"#;
        assert_eq!(
            partial_summary(reply).as_deref(),
            Some("line one\nline \"two\"")
        );
    }

    #[test]
    fn partial_summary_ignores_a_brace_before_the_field() {
        // A meta object earlier in the reply must not confuse the reader.
        let reply = r#"{"meta": {"summary": "wrong"}, "summary": "right"}"#;
        assert_eq!(partial_summary(reply).as_deref(), Some("wrong"));
    }
}
