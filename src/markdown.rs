//! Renders Markdown into native GTK widgets: Pango-markup labels for text and
//! syntax-highlighted, read-only source views with a copy button for code blocks.

use adw::prelude::*;
use gtk::glib;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use sourceview5::prelude::*;
use std::rc::Rc;

pub type CopyFn = Rc<dyn Fn(&str)>;

/// Maps common code-fence names to GtkSourceView language ids.
pub fn language_for(fence: &str) -> Option<sourceview5::Language> {
    let name = fence
        .split(|c: char| c == ',' || c.is_whitespace() || c == '{')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if name.is_empty() {
        return None;
    }
    let id = match name.as_str() {
        "rs" => "rust",
        "py" | "python" => "python3",
        "js" | "javascript" | "node" => "js",
        "ts" => "typescript",
        "sh" | "bash" | "shell" | "zsh" | "console" => "sh",
        "yml" => "yaml",
        "md" => "markdown",
        "c++" | "cc" | "hpp" => "cpp",
        "cs" | "csharp" => "c-sharp",
        "golang" => "go",
        "rb" => "ruby",
        "kt" => "kotlin",
        "dockerfile" | "docker" => "dockerfile",
        "html" | "xml" => name.as_str(),
        "jsonc" => "json",
        "ini" | "conf" => "ini",
        other => other,
    };
    sourceview5::LanguageManager::default().language(id)
}

/// Best available style scheme for the current light/dark mode.
pub fn style_scheme() -> Option<sourceview5::StyleScheme> {
    let dark = adw::StyleManager::default().is_dark();
    let ids: &[&str] = if dark {
        &["Adwaita-dark", "Yaru-dark", "oblivion", "classic-dark"]
    } else {
        &["Adwaita", "Yaru", "classic"]
    };
    let mgr = sourceview5::StyleSchemeManager::default();
    ids.iter().find_map(|id| mgr.scheme(id))
}

struct Renderer {
    root: gtk::Box,
    markup: String,
    lists: Vec<Option<u64>>,
    quote_depth: usize,
    code: Option<(String, String)>,
    in_table: bool,
    table_cell: usize,
    on_copy: CopyFn,
}

impl Renderer {
    fn push_text(&mut self, text: &str) {
        self.markup.push_str(&glib::markup_escape_text(text));
    }

    fn indent(&self) -> i32 {
        (self.lists.len().saturating_sub(1) * 20 + self.quote_depth * 14) as i32
    }

    /// Turns accumulated inline markup into a label.
    fn flush(&mut self) {
        let markup = std::mem::take(&mut self.markup);
        let markup = markup.trim_end();
        if markup.trim().is_empty() {
            return;
        }
        let label = gtk::Label::builder()
            .use_markup(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .selectable(true)
            .margin_start(self.indent())
            .build();
        label.set_markup(markup);
        if self.quote_depth > 0 {
            label.add_css_class("md-quote");
        }
        if self.in_table {
            label.add_css_class("monospace");
        }
        self.root.append(&label);
    }

    fn code_block(&mut self, lang: &str, code: &str) {
        let code = code.strip_suffix('\n').unwrap_or(code);
        let buffer = sourceview5::Buffer::new(None::<&gtk::TextTagTable>);
        buffer.set_text(code);
        buffer.set_highlight_syntax(true);
        if let Some(l) = language_for(lang) {
            buffer.set_language(Some(&l));
        }
        buffer.set_style_scheme(style_scheme().as_ref());

        let view = sourceview5::View::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(4)
            .bottom_margin(12)
            .left_margin(12)
            .right_margin(12)
            .build();
        view.add_css_class("code-view");

        // Header row: language on the left, copy button on the right.
        let lang_name = lang.split([',', ' ']).next().unwrap_or("");
        let tag = gtk::Label::builder()
            .label(if lang_name.is_empty() { "code" } else { lang_name })
            .xalign(0.0)
            .hexpand(true)
            .build();
        tag.add_css_class("caption");
        tag.add_css_class("dim-label");
        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy code")
            .build();
        copy.add_css_class("flat");
        let on_copy = self.on_copy.clone();
        let text = code.to_string();
        copy.connect_clicked(move |_| on_copy(&text));
        let header = gtk::Box::builder()
            .margin_start(12)
            .margin_end(4)
            .margin_top(2)
            .build();
        header.append(&tag);
        header.append(&copy);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("card");
        frame.set_margin_start(self.indent());
        frame.append(&header);
        frame.append(&view);
        self.root.append(&frame);
    }

    fn event(&mut self, ev: Event) {
        if let Some((_, code)) = &mut self.code {
            match ev {
                Event::Text(t) => code.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, code) = self.code.take().unwrap();
                    self.code_block(&lang, &code);
                }
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    self.flush();
                    let size = match level {
                        HeadingLevel::H1 => "xx-large",
                        HeadingLevel::H2 => "x-large",
                        HeadingLevel::H3 => "large",
                        _ => "medium",
                    };
                    self.markup
                        .push_str(&format!("<span size=\"{size}\" weight=\"bold\">"));
                }
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.quote_depth += 1;
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    self.flush();
                    self.lists.push(start);
                }
                Tag::Item => {
                    self.flush();
                    let bullet = match self.lists.last_mut() {
                        Some(Some(n)) => {
                            let b = format!("{n}. ");
                            *n += 1;
                            b
                        }
                        _ => "•  ".to_string(),
                    };
                    self.markup.push_str(&bullet);
                }
                Tag::Emphasis => self.markup.push_str("<i>"),
                Tag::Strong => self.markup.push_str("<b>"),
                Tag::Strikethrough => self.markup.push_str("<s>"),
                Tag::Link { dest_url, .. } => {
                    let href = glib::markup_escape_text(&dest_url);
                    self.markup.push_str(&format!("<a href=\"{href}\">"));
                }
                Tag::Table(_) => {
                    self.flush();
                    self.in_table = true;
                }
                Tag::TableHead | Tag::TableRow => {
                    self.table_cell = 0;
                    if matches!(tag, Tag::TableHead) {
                        self.markup.push_str("<b>");
                    }
                }
                Tag::TableCell => {
                    if self.table_cell > 0 {
                        self.markup.push_str("  │  ");
                    }
                    self.table_cell += 1;
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                // In list items the bullet was pushed at Item start, so it stays with this text.
                TagEnd::Paragraph => self.flush(),
                TagEnd::Heading(_) => {
                    self.markup.push_str("</span>");
                    self.flush();
                }
                TagEnd::BlockQuote(_) => {
                    self.flush();
                    self.quote_depth = self.quote_depth.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                }
                TagEnd::Item => self.flush(),
                TagEnd::Emphasis => self.markup.push_str("</i>"),
                TagEnd::Strong => self.markup.push_str("</b>"),
                TagEnd::Strikethrough => self.markup.push_str("</s>"),
                TagEnd::Link => self.markup.push_str("</a>"),
                TagEnd::TableHead => {
                    self.markup.push_str("</b>\n");
                }
                TagEnd::TableRow => self.markup.push('\n'),
                TagEnd::Table => {
                    self.flush();
                    self.in_table = false;
                }
                _ => {}
            },
            Event::Text(t) => self.push_text(&t),
            Event::Code(c) => {
                self.markup.push_str(
                    "<span font_family=\"monospace\" bgcolor=\"#808080\" bgalpha=\"22%\">",
                );
                self.push_text(&c);
                self.markup.push_str("</span>");
            }
            Event::InlineMath(m) | Event::DisplayMath(m) => self.push_text(&m),
            Event::Html(h) | Event::InlineHtml(h) => self.push_text(&h),
            Event::SoftBreak => self.markup.push(' '),
            Event::HardBreak => self.markup.push('\n'),
            Event::Rule => {
                self.flush();
                let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
                sep.set_margin_top(6);
                sep.set_margin_bottom(6);
                self.root.append(&sep);
            }
            Event::TaskListMarker(done) => {
                if self.markup.ends_with("•  ") {
                    self.markup.truncate(self.markup.len() - "•  ".len());
                }
                self.markup.push_str(if done { "☑  " } else { "☐  " });
            }
            Event::FootnoteReference(f) => self.push_text(&format!("[{f}]")),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

/// Fills `root` (cleared first) with widgets rendering `markdown`.
pub fn render_into(root: &gtk::Box, markdown: &str, on_copy: CopyFn) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer {
        root: root.clone(),
        markup: String::new(),
        lists: Vec::new(),
        quote_depth: 0,
        code: None,
        in_table: false,
        table_cell: 0,
        on_copy,
    };
    for ev in Parser::new_ext(markdown, opts) {
        r.event(ev);
    }
    r.flush();
    if r.root.first_child().is_none() {
        let empty = gtk::Label::new(Some("Nothing to preview yet."));
        empty.add_css_class("dim-label");
        empty.set_vexpand(true);
        root.append(&empty);
    }
}
