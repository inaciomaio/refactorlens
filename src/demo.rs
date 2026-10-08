//! A built-in example with a hand-written answer.
//! It lets people (and contributors) try the whole interface without a model.

use serde_json::json;

pub const SAMPLE_CODE: &str = r#"import os

def load_scores(path):
    f = open(path, "r")
    lines = f.readlines()
    f.close()
    scores = {}
    for i in range(len(lines)):
        line = lines[i].strip()
        if line == "":
            continue
        parts = line.split(",")
        name = parts[0]
        score = int(parts[1])
        if name in scores:
            scores[name].append(score)
        else:
            scores[name] = [score]
    return scores

def average(nums):
    total = 0
    for n in nums:
        total = total + n
    return total / len(nums)

def report(path):
    if os.path.exists(path) == False:
        print("File not found: " + path)
        return
    scores = load_scores(path)
    for name in scores.keys():
        avg = average(scores[name])
        print(name + ": " + str(round(avg, 2)))
"#;

const IMPROVED: &str = r#"from collections import defaultdict
from pathlib import Path
from statistics import mean


def load_scores(path: Path) -> dict[str, list[int]]:
    scores: defaultdict[str, list[int]] = defaultdict(list)
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            name, score = line.split(",", maxsplit=1)
            scores[name].append(int(score))
    return dict(scores)


def report(path: str | Path) -> None:
    path = Path(path)
    if not path.exists():
        print(f"File not found: {path}")
        return
    for name, values in load_scores(path).items():
        print(f"{name}: {mean(values):.2f}")
"#;

/// True when the submitted code is the built-in example (ignoring whitespace at the ends).
pub fn is_sample(code: &str) -> bool {
    code.replace("\r\n", "\n").trim() == SAMPLE_CODE.trim()
}

/// The canned reply, as raw text, exactly like a model would send it.
pub fn reply() -> String {
    json!({
        "language": "python",
        "summary": "This script reads name,score lines from a file and prints each person's average. The rewrite keeps the same behaviour but leans on modern Python: pathlib for paths, a context manager for the file, defaultdict for grouping, and f-strings for output. It also adds type hints so editors can catch mistakes early.",
        "improved_code": IMPROVED,
        "changes": [
            {
                "title": "Add type hints",
                "category": "safety",
                "before_snippet": "def load_scores(path):",
                "after_snippet": "def load_scores(path: Path) -> dict[str, list[int]]:",
                "what": "The function now declares that it takes a Path and returns a dict of name to list of ints.",
                "why": "Type hints are documentation the computer can check. Your editor and tools like mypy can now warn you if you pass the wrong thing or misuse the result.",
                "concept": { "name": "Type hints", "explanation": "Annotations like `x: int` or `-> str` describe what a value should be. Python ignores them at runtime, but checkers and editors use them to find bugs before you run the code." }
            },
            {
                "title": "Open files with a context manager",
                "category": "safety",
                "before_snippet": "    f = open(path, \"r\")\n    lines = f.readlines()\n    f.close()",
                "after_snippet": "    with path.open(encoding=\"utf-8\") as f:",
                "what": "The manual open/close pair became a `with` block.",
                "why": "If anything between open() and close() raises an error, the old version leaves the file open. `with` closes it no matter what. Naming the encoding also stops the result from depending on the computer's default.",
                "concept": { "name": "Context managers", "explanation": "A `with` block runs setup code on entry and cleanup code on exit, even when an exception happens. Files, locks and database connections all use this pattern." }
            },
            {
                "title": "Loop over items, not indexes",
                "category": "readability",
                "before_snippet": "    for i in range(len(lines)):\n        line = lines[i].strip()",
                "after_snippet": "        for line in f:\n            line = line.strip()",
                "what": "The loop walks the file's lines directly instead of counting indexes.",
                "why": "`range(len(...))` adds a variable you never needed. Iterating the file also reads one line at a time, so huge files no longer have to fit in memory.",
                "concept": { "name": "Direct iteration", "explanation": "In Python, `for x in thing` works on any iterable. Reach for indexes only when you truly need the position; use `enumerate()` if you need both." }
            },
            {
                "title": "Test emptiness with truthiness",
                "category": "readability",
                "before_snippet": "        if line == \"\":",
                "after_snippet": "            if not line:",
                "what": "The comparison with an empty string became `not line`.",
                "why": "Empty strings, lists and dicts are all 'falsy'. `if not line` reads naturally and is the style most Python developers expect.",
                "concept": { "name": "Truthiness", "explanation": "Every Python value can act as a boolean. Empty containers, 0 and None count as False; almost everything else counts as True." }
            },
            {
                "title": "Unpack instead of indexing",
                "category": "readability",
                "before_snippet": "        parts = line.split(\",\")\n        name = parts[0]\n        score = int(parts[1])",
                "after_snippet": "            name, score = line.split(\",\", maxsplit=1)",
                "what": "Three lines that pulled values out by position became one unpacking assignment.",
                "why": "Unpacking names each piece directly. `maxsplit=1` makes the intent clear: split once into a name and a score.",
                "concept": { "name": "Tuple unpacking", "explanation": "`a, b = pair` assigns each element to its own name. It raises an error if the counts don't match, which catches malformed data early." }
            },
            {
                "title": "Group values with defaultdict",
                "category": "modern-syntax",
                "before_snippet": "        if name in scores:\n            scores[name].append(score)\n        else:\n            scores[name] = [score]",
                "after_snippet": "            scores[name].append(int(score))",
                "what": "The if/else that created lists on first sight is gone.",
                "why": "`defaultdict(list)` creates an empty list the first time a key is used, so every line can simply append. Less branching, fewer places for bugs.",
                "concept": { "name": "defaultdict", "explanation": "A dict from the `collections` module that builds a default value for missing keys using the factory you give it, such as `list`, `int` or `set`." }
            },
            {
                "title": "Use statistics.mean",
                "category": "readability",
                "before_snippet": "def average(nums):\n    total = 0\n    for n in nums:\n        total = total + n\n    return total / len(nums)",
                "after_snippet": "from statistics import mean",
                "what": "The hand-written average function was replaced by the standard library.",
                "why": "Code you don't write is code you don't have to test. `mean` is well tested and handles edge cases such as mixing ints and floats precisely.",
                "concept": { "name": "Know the standard library", "explanation": "Before writing a helper, check whether the language already ships one. Python's `statistics`, `itertools` and `collections` modules cover many everyday tasks." }
            },
            {
                "title": "Use pathlib for file paths",
                "category": "modern-syntax",
                "before_snippet": "    if os.path.exists(path) == False:",
                "after_snippet": "    if not path.exists():",
                "what": "The os.path check became a method on a Path object, and `== False` became `not`.",
                "why": "Path objects keep path logic in one place and work the same on Windows, macOS and Linux. Comparing to False is a classic beginner pattern that linters flag.",
                "concept": { "name": "pathlib", "explanation": "`pathlib.Path` represents a filesystem path as an object with methods like `.exists()`, `.open()` and `.read_text()`, replacing most of `os.path`." }
            },
            {
                "title": "Iterate pairs with .items()",
                "category": "readability",
                "before_snippet": "    for name in scores.keys():\n        avg = average(scores[name])",
                "after_snippet": "    for name, values in load_scores(path).items():",
                "what": "The loop gets each name and its scores together.",
                "why": "Looking each key up again with `scores[name]` is extra work and extra noise. `.items()` hands you both at once.",
                "concept": { "name": "dict.items()", "explanation": "Iterating `d.items()` yields `(key, value)` pairs, which you can unpack straight into two loop variables." }
            },
            {
                "title": "Format output with f-strings",
                "category": "modern-syntax",
                "before_snippet": "        print(name + \": \" + str(round(avg, 2)))",
                "after_snippet": "        print(f\"{name}: {mean(values):.2f}\")",
                "what": "String concatenation became a single f-string with a format spec.",
                "why": "f-strings show the final shape of the text at a glance and convert values for you. `:.2f` always prints two decimals, which looks tidier in a report.",
                "concept": { "name": "f-strings", "explanation": "Strings prefixed with `f` can embed expressions in braces. A format spec after a colon, like `:.2f` or `:>10`, controls how the value is printed." }
            }
        ],
        "dependencies": [],
        "behavior_changes": [
            "Averages now always print with two decimals, for example 90.00 instead of 90.0.",
            "The file is read as UTF-8 instead of the system's default encoding."
        ]
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{extract_json, resolve_changes};

    /// Every snippet in the demo must point at real lines, or the UI links break.
    #[test]
    fn every_demo_snippet_resolves() {
        let parsed = extract_json(&reply()).unwrap();
        let changes = resolve_changes(SAMPLE_CODE, &parsed.improved_code, parsed.changes);
        for c in changes {
            assert!(c.before_lines.is_some(), "before not found: {}", c.title);
            assert!(c.after_lines.is_some(), "after not found: {}", c.title);
        }
    }
}
