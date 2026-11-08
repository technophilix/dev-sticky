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

#[cfg(test)]
mod tests {
    use super::*;

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
