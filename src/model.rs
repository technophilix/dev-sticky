//! Plain data types and text helpers. No GTK in here so it stays unit-testable.

pub const COLORS: &[(&str, &str)] = &[
    ("yellow", "Yellow"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("purple", "Purple"),
    ("pink", "Pink"),
    ("orange", "Orange"),
];

pub const DEFAULT_COLOR: &str = "yellow";

/// Returns `color` if it is one we know, otherwise the default.
pub fn normalize_color(color: &str) -> &'static str {
    COLORS
        .iter()
        .find(|(id, _)| *id == color)
        .map(|(id, _)| *id)
        .unwrap_or(DEFAULT_COLOR)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub color: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Lightweight row used by the lazily loaded list.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub tags: Vec<String>,
    pub color: String,
    pub pinned: bool,
    pub updated_at: i64,
    /// (done, total) checkbox items, for showing to-do progress.
    pub tasks: (usize, usize),
}

pub const UNTITLED: &str = "Untitled note";

fn is_fence(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("```") || l.starts_with("~~~")
}

/// Index of the line used as the title: first non-empty line that isn't a code fence.
fn title_line(body: &str) -> Option<(usize, &str)> {
    body.lines()
        .enumerate()
        .find(|(_, l)| !l.trim().is_empty() && !is_fence(l))
}

/// Derives a title from the first meaningful line of the Markdown body.
pub fn derive_title(body: &str) -> String {
    let Some((_, line)) = title_line(body) else {
        return UNTITLED.to_string();
    };
    let t = line
        .trim()
        .trim_start_matches('#')
        .trim_start_matches(['-', '*', '>', ' '])
        .trim();
    if t.is_empty() {
        return UNTITLED.to_string();
    }
    truncate(t, 80)
}

/// A one-line preview of the body, skipping the title line.
pub fn snippet(body: &str) -> String {
    let skip = title_line(body).map(|(i, _)| i);
    let mut out = String::new();
    for (i, line) in body.lines().enumerate() {
        if Some(i) == skip || is_fence(line) {
            continue;
        }
        let l = line.trim().trim_start_matches(['#', '-', '*', '>']).trim();
        let l = l
            .strip_prefix("[ ] ")
            .or_else(|| l.strip_prefix("[x] "))
            .or_else(|| l.strip_prefix("[X] "))
            .unwrap_or(l)
            .replace("**", "")
            .replace('`', "");
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(l);
        if out.chars().count() > 140 {
            break;
        }
    }
    truncate(&out, 140)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

/// Parses user tag input like "#bug, Rust  todo" into ["bug", "rust", "todo"].
pub fn parse_tags(input: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for raw in input.split(|c: char| c == ',' || c.is_whitespace()) {
        let tag: String = raw
            .trim()
            .trim_start_matches('#')
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || "-_./+".contains(*c))
            .collect();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

pub fn format_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// "just now", "5m ago", "3h ago", "2d ago", or a date.
pub fn relative_time(ts: i64, now: i64) -> String {
    let d = (now - ts).max(0);
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", d / 60),
        3600..86400 => format!("{}h ago", d / 3600),
        86400..604800 => format!("{}d ago", d / 86400),
        _ => chrono::DateTime::from_timestamp(ts, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string())
            .unwrap_or_default(),
    }
}

/// Body for "New To-do List" notes. The cursor goes at the end, on the first item.
pub const TODO_TEMPLATE: &str = "# To-do\n\n- [ ] ";

/// A Markdown list line split into parts, e.g. "  - [x] buy milk" →
/// indent "  ", marker "-", task Some(true), rest "buy milk".
struct ListLine<'a> {
    indent: &'a str,
    marker: &'a str,
    task: Option<bool>,
    rest: &'a str,
}

fn parse_list_line(line: &str) -> Option<ListLine<'_>> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let marker_len = if body.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = body.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || !body[digits..].starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    let marker = &body[..marker_len];
    let after = &body[marker_len..];
    // A marker must be followed by a space, or end the line ("-" alone).
    let after = match after.strip_prefix(' ') {
        Some(a) => a,
        None if after.is_empty() => after,
        None => return None,
    };
    let (task, rest) = match after.get(..3) {
        Some("[ ]") => (Some(false), &after[3..]),
        Some("[x]" | "[X]") => (Some(true), &after[3..]),
        _ => (None, after),
    };
    // "[x]" must be followed by a space or the end of the line to be a checkbox.
    let (task, rest) = match (task, rest.strip_prefix(' ')) {
        (Some(t), Some(r)) => (Some(t), r),
        (Some(t), None) if rest.is_empty() => (Some(t), rest),
        (Some(_), None) => (None, after),
        (None, _) => (None, rest),
    };
    Some(ListLine { indent, marker, task, rest })
}

/// Ctrl+T on a line: tick / untick a checkbox, or turn the line into one.
pub fn toggle_task_line(line: &str) -> String {
    match parse_list_line(line) {
        Some(l) => match l.task {
            Some(done) => {
                let mark = if done { "[ ]" } else { "[x]" };
                format!("{}{} {mark} {}", l.indent, l.marker, l.rest)
            }
            None => format!("{}{} [ ] {}", l.indent, l.marker, l.rest),
        },
        None => {
            let text = line.trim_start();
            let indent = &line[..line.len() - text.len()];
            format!("{indent}- [ ] {text}")
        }
    }
}

/// What pressing Enter at the end of a list line should do.
#[derive(Debug, PartialEq)]
pub enum ListEnter {
    /// Start a new item: insert a newline followed by this prefix.
    Continue(String),
    /// The item was empty: clear this line's marker, ending the list.
    EndList,
}

pub fn list_enter(line: &str) -> Option<ListEnter> {
    let l = parse_list_line(line)?;
    if l.rest.trim().is_empty() {
        return Some(ListEnter::EndList);
    }
    let marker = match l.marker.strip_suffix(['.', ')']) {
        Some(n) => {
            let next = n.parse::<u64>().map(|n| n + 1).unwrap_or(1);
            format!("{next}{}", &l.marker[n.len()..])
        }
        None => l.marker.to_string(),
    };
    let task = if l.task.is_some() { "[ ] " } else { "" };
    Some(ListEnter::Continue(format!("{}{marker} {task}", l.indent)))
}

/// (done, total) checkbox items in a note, ignoring code blocks.
pub fn task_counts(body: &str) -> (usize, usize) {
    let (mut done, mut total, mut in_code) = (0, 0, false);
    for line in body.lines() {
        if is_fence(line) {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some(ListLine { task: Some(t), .. }) = parse_list_line(line) {
            total += 1;
            done += usize::from(t);
        }
    }
    (done, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_task_lines() {
        assert_eq!(toggle_task_line("- [ ] milk"), "- [x] milk");
        assert_eq!(toggle_task_line("  * [X] eggs"), "  * [ ] eggs");
        assert_eq!(toggle_task_line("- bread"), "- [ ] bread");
        assert_eq!(toggle_task_line("1. step"), "1. [ ] step");
        assert_eq!(toggle_task_line("  call mom"), "  - [ ] call mom");
        assert_eq!(toggle_task_line(""), "- [ ] ");
        assert_eq!(toggle_task_line("- [ ] "), "- [x] ");
        assert_eq!(toggle_task_line("-flag"), "- [ ] -flag");
    }

    #[test]
    fn enter_continues_lists() {
        let c = |s: &str| Some(ListEnter::Continue(s.to_string()));
        assert_eq!(list_enter("- [x] done thing"), c("- [ ] "));
        assert_eq!(list_enter("  - item"), c("  - "));
        assert_eq!(list_enter("9. nine"), c("10. "));
        assert_eq!(list_enter("3) [ ] task"), c("4) [ ] "));
        assert_eq!(list_enter("- [ ] "), Some(ListEnter::EndList));
        assert_eq!(list_enter("- "), Some(ListEnter::EndList));
        assert_eq!(list_enter("plain text"), None);
        assert_eq!(list_enter("-not a list"), None);
    }

    #[test]
    fn counting_tasks() {
        let body = "# To-do\n- [x] a\n- [ ] b\n  - [X] c\n```md\n- [ ] in code\n```\n- [link] no";
        assert_eq!(task_counts(body), (2, 3));
        assert_eq!(task_counts(TODO_TEMPLATE), (0, 1));
    }

    #[test]
    fn title_from_heading_and_fence() {
        assert_eq!(derive_title("# Fix auth bug\nmore"), "Fix auth bug");
        assert_eq!(derive_title("\n\n```rust\nfn main() {}\n```"), "fn main() {}");
        assert_eq!(derive_title("   \n"), UNTITLED);
        assert_eq!(derive_title("- [ ] todo item"), "[ ] todo item");
    }

    #[test]
    fn snippet_skips_title() {
        assert_eq!(snippet("# Title\n\nFirst line\n```sh\nls -la\n```"), "First line ls -la");
        assert_eq!(snippet("T\nIt is **bold** and `code`\n- [x] done"), "It is bold and code done");
    }

    #[test]
    fn tags_are_normalized() {
        assert_eq!(parse_tags("#Bug, rust  rust,, todo!"), vec!["bug", "rust", "todo"]);
        assert_eq!(parse_tags("c++ node.js"), vec!["c++", "node.js"]);
    }

    #[test]
    fn colors() {
        assert_eq!(normalize_color("blue"), "blue");
        assert_eq!(normalize_color("rainbow"), DEFAULT_COLOR);
    }

    #[test]
    fn relative() {
        assert_eq!(relative_time(100, 110), "just now");
        assert_eq!(relative_time(0, 7200), "2h ago");
    }
}
