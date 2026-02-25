use crate::render::line_utils::line_to_static;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use comfy_table::CellAlignment;
use comfy_table::ContentArrangement;
use comfy_table::Table;
use pulldown_cmark::Alignment as CmarkAlignment;
use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use unicode_width::UnicodeWidthStr;

use crate::render::highlight::HighlightLanguage;

struct MarkdownStyles {
    h1: Style,
    h2: Style,
    h3: Style,
    h4: Style,
    h5: Style,
    h6: Style,
    code: Style,
    emphasis: Style,
    strong: Style,
    strikethrough: Style,
    ordered_list_marker: Style,
    unordered_list_marker: Style,
    link: Style,
    blockquote: Style,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        use ratatui::style::Stylize;

        Self {
            h1: Style::new().bold().underlined(),
            h2: Style::new().bold(),
            h3: Style::new().bold().italic(),
            h4: Style::new().italic(),
            h5: Style::new().italic(),
            h6: Style::new().italic(),
            code: Style::new().cyan(),
            emphasis: Style::new().italic(),
            strong: Style::new().bold(),
            strikethrough: Style::new().crossed_out(),
            ordered_list_marker: Style::new().light_blue(),
            unordered_list_marker: Style::new(),
            link: Style::new().cyan().underlined(),
            blockquote: Style::new().green(),
        }
    }
}

static TABLES_ENABLED: AtomicBool = AtomicBool::new(false);
const UTF8_TABLE_PRESET: &str = "││──├─┼┤│─┼├┤┬┴┌┐└┘";
const TABLE_MAX_WIDTH_FALLBACK: usize = 160;
const TABLE_MIN_WIDTH: usize = 10;

pub(crate) fn set_tables_enabled(enabled: bool) {
    TABLES_ENABLED.store(enabled, Ordering::Relaxed);
}

pub(crate) fn tables_enabled() -> bool {
    TABLES_ENABLED.load(Ordering::Relaxed)
}

#[derive(Clone, Debug)]
struct IndentContext {
    prefix: Vec<Span<'static>>,
    marker: Option<Vec<Span<'static>>>,
    is_list: bool,
}

impl IndentContext {
    fn new(prefix: Vec<Span<'static>>, marker: Option<Vec<Span<'static>>>, is_list: bool) -> Self {
        Self {
            prefix,
            marker,
            is_list,
        }
    }
}

#[derive(Clone, Debug)]
struct TableState {
    alignments: Vec<CmarkAlignment>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    header_rows: usize,
    in_head: bool,
    row_open: bool,
}

impl TableState {
    fn new(alignments: Vec<CmarkAlignment>) -> Self {
        Self {
            alignments,
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            header_rows: 0,
            in_head: false,
            row_open: false,
        }
    }

    fn start_row(&mut self) {
        self.current_row = Vec::new();
        self.current_cell.clear();
        self.row_open = true;
    }

    fn end_row(&mut self) {
        if !self.current_cell.is_empty() {
            self.current_row.push(self.current_cell.trim().to_string());
            self.current_cell.clear();
        }
        if !self.current_row.is_empty() {
            self.rows.push(std::mem::take(&mut self.current_row));
            if self.in_head {
                self.header_rows = self.header_rows.saturating_add(1);
            }
        }
        self.row_open = false;
    }

    fn start_cell(&mut self) {
        self.current_cell.clear();
    }

    fn end_cell(&mut self) {
        self.current_row.push(self.current_cell.trim().to_string());
        self.current_cell.clear();
    }

    fn push_text(&mut self, text: &str) {
        if !self.current_cell.is_empty() {
            self.current_cell.push_str(text);
            return;
        }
        self.current_cell = text.to_string();
    }

    fn push_space(&mut self) {
        if !self.current_cell.ends_with(' ') {
            self.current_cell.push(' ');
        }
    }
}

pub fn render_markdown_text(input: &str) -> Text<'static> {
    render_markdown_text_with_width(input, None)
}

pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let tables_on = tables_enabled();
    if tables_on {
        options.insert(Options::ENABLE_TABLES);
    }
    let normalized = if tables_on {
        Cow::Owned(normalize_table_blocks(input))
    } else {
        Cow::Borrowed(input)
    };
    let parser = Parser::new_ext(normalized.as_ref(), options);
    let mut w = Writer::new(parser, width, tables_on);
    w.run();
    w.text
}

fn normalize_table_blocks(input: &str) -> String {
    let ends_with_newline = input.ends_with('\n');
    let lines: Vec<&str> = input.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx];
        if is_box_table_line(line) {
            idx += 1;
            continue;
        }

        let Some((strip_len, content)) = split_table_prefix(line) else {
            out.push(line.to_string());
            idx += 1;
            continue;
        };

        if !is_pipe_table_line(content) {
            out.push(line.to_string());
            idx += 1;
            continue;
        }

        let mut rows: Vec<String> = Vec::new();
        rows.push(strip_prefix(line, strip_len));
        idx += 1;

        while idx < lines.len() {
            let line = lines[idx];
            if line.trim().is_empty() {
                idx += 1;
                break;
            }
            if is_box_table_line(line) {
                idx += 1;
                continue;
            }
            let stripped = strip_prefix(line, strip_len);
            if is_pipe_table_line(&stripped) || is_pipe_table_separator(&stripped) {
                rows.push(stripped);
                idx += 1;
                continue;
            }
            break;
        }

        if !rows.iter().any(|row| is_pipe_table_separator(row))
            && let Some(separator) = build_pipe_table_separator(&rows)
        {
            rows.insert(1, separator);
        }

        out.extend(rows);
        out.push(String::new());
    }

    let mut normalized = out.join("\n");
    if ends_with_newline {
        normalized.push('\n');
    }
    normalized
}

fn split_table_prefix(line: &str) -> Option<(usize, &str)> {
    if line.is_empty() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut idx = 0usize;
    let len = bytes.len();
    while idx < len && matches!(bytes[idx], b' ' | b'\t') {
        idx += 1;
    }
    while idx < len && bytes[idx] == b'>' {
        idx += 1;
        if idx < len && bytes[idx] == b' ' {
            idx += 1;
        }
        while idx < len && matches!(bytes[idx], b' ' | b'\t') {
            idx += 1;
        }
    }
    let marker_start = idx;
    if idx < len {
        let rest = &line[idx..];
        if rest.starts_with('\u{2022}') {
            idx += '\u{2022}'.len_utf8();
        } else {
            match bytes[idx] {
                b'-' | b'+' | b'*' => {
                    idx += 1;
                }
                _ if bytes[idx].is_ascii_digit() => {
                    while idx < len && bytes[idx].is_ascii_digit() {
                        idx += 1;
                    }
                    if idx < len && matches!(bytes[idx], b'.' | b')') {
                        idx += 1;
                    } else {
                        idx = marker_start;
                    }
                }
                _ => {}
            }
        }
    }
    if idx != marker_start {
        if idx < len && bytes[idx] == b' ' {
            idx += 1;
        }
        while idx < len && matches!(bytes[idx], b' ' | b'\t') {
            idx += 1;
        }
    }

    Some((idx, &line[idx..]))
}

fn strip_prefix(line: &str, strip_len: usize) -> String {
    if line.len() <= strip_len {
        return String::new();
    }
    line[strip_len..].to_string()
}

fn is_pipe_table_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.is_empty() && !is_pipe_table_separator(trimmed)
}

fn is_pipe_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.contains('|') {
        return false;
    }
    let mut has_dash = false;
    for ch in trimmed.chars() {
        match ch {
            '-' => has_dash = true,
            '|' | ':' | ' ' | '\t' => {}
            _ => return false,
        }
    }
    has_dash
}

fn build_pipe_table_separator(rows: &[String]) -> Option<String> {
    let header = rows.iter().find(|row| is_pipe_table_line(row))?;
    let trimmed = header.trim();
    let mut parts: Vec<&str> = trimmed.split('|').collect();
    // Drop empty columns introduced by leading/trailing pipes.
    if trimmed.starts_with('|') && !parts.is_empty() {
        parts.remove(0);
    }
    if trimmed.ends_with('|') && !parts.is_empty() {
        parts.pop();
    }
    // Count columns including empty header cells; this is important when we
    // synthesize a missing separator row. Otherwise tables with blank header
    // cells (e.g. a row-number column) fail to parse as tables.
    let col_count = parts.len();
    if col_count == 0 {
        return None;
    }
    let mut separator = String::from("|");
    for _ in 0..col_count {
        separator.push_str(" --- |");
    }
    Some(separator)
}

struct Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    iter: I,
    text: Text<'static>,
    styles: MarkdownStyles,
    inline_styles: Vec<Style>,
    indent_stack: Vec<IndentContext>,
    list_indices: Vec<Option<u64>>,
    link: Option<String>,
    needs_newline: bool,
    pending_marker_line: bool,
    in_paragraph: bool,
    in_code_block: bool,
    wrap_width: Option<usize>,
    tables_enabled: bool,
    table_state: Option<TableState>,
    current_line_content: Option<Line<'static>>,
    current_initial_indent: Vec<Span<'static>>,
    current_subsequent_indent: Vec<Span<'static>>,
    current_line_style: Style,
    current_line_in_code_block: bool,
    buffered_code_block: Option<BufferedCodeBlock>,
}

#[derive(Clone, Debug)]
struct BufferedCodeBlock {
    lang: HighlightLanguage,
    lines: Vec<String>,
}

impl<'a, I> Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn new(iter: I, wrap_width: Option<usize>, tables_enabled: bool) -> Self {
        Self {
            iter,
            text: Text::default(),
            styles: MarkdownStyles::default(),
            inline_styles: Vec::new(),
            indent_stack: Vec::new(),
            list_indices: Vec::new(),
            link: None,
            needs_newline: false,
            pending_marker_line: false,
            in_paragraph: false,
            in_code_block: false,
            wrap_width,
            tables_enabled,
            table_state: None,
            current_line_content: None,
            current_initial_indent: Vec::new(),
            current_subsequent_indent: Vec::new(),
            current_line_style: Style::default(),
            current_line_in_code_block: false,
            buffered_code_block: None,
        }
    }

    fn run(&mut self) {
        while let Some(ev) = self.iter.next() {
            self.handle_event(ev);
        }
        self.flush_current_line();
    }

    fn handle_event(&mut self, event: Event<'a>) {
        if self.table_state.is_some() && self.handle_table_event(&event) {
            return;
        }
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text),
            Event::Code(code) => self.code(code),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => {
                self.flush_current_line();
                if !self.text.lines.is_empty() {
                    self.push_blank_line();
                }
                self.push_line(Line::from("———"));
                self.needs_newline = true;
            }
            Event::Html(html) => self.html(html, false),
            Event::InlineHtml(html) => self.html(html, true),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote => self.start_blockquote(),
            Tag::CodeBlock(kind) => {
                let indent = match kind {
                    CodeBlockKind::Fenced(_) => None,
                    CodeBlockKind::Indented => Some(Span::from(" ".repeat(4))),
                };
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    CodeBlockKind::Indented => None,
                };
                self.start_codeblock(lang, indent)
            }
            Tag::List(start) => self.start_list(start),
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_inline_style(self.styles.emphasis),
            Tag::Strong => self.push_inline_style(self.styles.strong),
            Tag::Strikethrough => self.push_inline_style(self.styles.strikethrough),
            Tag::Link { dest_url, .. } => self.push_link(dest_url.to_string()),
            Tag::Table(alignments) => {
                if self.tables_enabled {
                    self.start_table(alignments);
                }
            }
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Image { .. }
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_codeblock(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => {
                self.indent_stack.pop();
                self.pending_marker_line = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline_style(),
            TagEnd::Link => self.pop_link(),
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Image
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.push_line(Line::default());
        self.needs_newline = false;
        self.in_paragraph = true;
    }

    fn end_paragraph(&mut self) {
        self.needs_newline = true;
        self.in_paragraph = false;
        self.pending_marker_line = false;
    }

    fn start_heading(&mut self, level: HeadingLevel) {
        if self.needs_newline {
            self.push_line(Line::default());
            self.needs_newline = false;
        }
        let heading_style = match level {
            HeadingLevel::H1 => self.styles.h1,
            HeadingLevel::H2 => self.styles.h2,
            HeadingLevel::H3 => self.styles.h3,
            HeadingLevel::H4 => self.styles.h4,
            HeadingLevel::H5 => self.styles.h5,
            HeadingLevel::H6 => self.styles.h6,
        };
        let content = format!("{} ", "#".repeat(level as usize));
        self.push_line(Line::from(vec![Span::styled(content, heading_style)]));
        self.push_inline_style(heading_style);
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        self.needs_newline = true;
        self.pop_inline_style();
    }

    fn start_table(&mut self, alignments: Vec<CmarkAlignment>) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.flush_current_line();
        self.in_paragraph = false;
        self.table_state = Some(TableState::new(alignments));
        self.needs_newline = false;
    }

    fn handle_table_event(&mut self, event: &Event<'a>) -> bool {
        let Some(table) = self.table_state.as_mut() else {
            return false;
        };
        match event {
            Event::Start(Tag::TableHead) => {
                table.in_head = true;
            }
            Event::End(TagEnd::TableHead) => {
                if table.row_open {
                    table.end_row();
                }
                table.in_head = false;
            }
            Event::Start(Tag::TableRow) => {
                table.start_row();
            }
            Event::End(TagEnd::TableRow) => {
                table.end_row();
            }
            Event::Start(Tag::TableCell) => {
                if !table.row_open {
                    table.start_row();
                }
                table.start_cell();
            }
            Event::End(TagEnd::TableCell) => {
                table.end_cell();
            }
            Event::End(TagEnd::Table) => {
                if table.row_open {
                    table.end_row();
                }
                self.finish_table();
            }
            Event::Text(text) => {
                table.push_text(text.as_ref());
            }
            Event::Code(code) => {
                table.push_text(code.as_ref());
            }
            Event::SoftBreak | Event::HardBreak => {
                table.push_space();
            }
            _ => {}
        }
        true
    }

    fn finish_table(&mut self) {
        let Some(table) = self.table_state.take() else {
            return;
        };
        self.render_table(table);
        self.needs_newline = true;
    }

    fn render_table(&mut self, table: TableState) {
        if table.rows.is_empty() {
            return;
        }

        let mut column_count = table.alignments.len();
        for row in &table.rows {
            column_count = column_count.max(row.len());
        }
        if column_count == 0 {
            return;
        }

        let mut rows = table.rows;
        for row in &mut rows {
            if row.len() < column_count {
                row.resize_with(column_count, String::new);
            }
        }

        let header = if table.header_rows > 0 && !rows.is_empty() {
            Some(rows.remove(0))
        } else {
            None
        };

        let mut table_output = Table::new();
        table_output.load_preset(UTF8_TABLE_PRESET);
        table_output.set_content_arrangement(ContentArrangement::Dynamic);

        // Hard cap the rendered table width to avoid terminal overflow. We prefer the
        // current markdown wrap width (computed from the TUI layout). If it's unavailable,
        // use the terminal width. If that fails too, fall back to a conservative constant.
        let max_width = self
            .wrap_width
            .or_else(terminal_width_cols)
            .unwrap_or(TABLE_MAX_WIDTH_FALLBACK);
        let prefix_width: usize = self
            .prefix_spans(self.pending_marker_line)
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        let available_width = max_width.saturating_sub(prefix_width).max(TABLE_MIN_WIDTH);
        table_output.set_width(available_width.min(u16::MAX as usize) as u16);

        if let Some(header) = header {
            table_output.set_header(header);
        }
        for row in rows {
            table_output.add_row(row);
        }

        for (idx, alignment) in table.alignments.iter().enumerate() {
            if let Some(column) = table_output.column_mut(idx) {
                let cell_alignment = match alignment {
                    CmarkAlignment::Right => CellAlignment::Right,
                    CmarkAlignment::Center => CellAlignment::Center,
                    CmarkAlignment::None | CmarkAlignment::Left => CellAlignment::Left,
                };
                column.set_cell_alignment(cell_alignment);
            }
        }

        let rendered = table_output.to_string();
        for line in rendered.lines() {
            self.push_line(Line::from(line.to_string()));
        }
        self.flush_current_line();
    }

    fn start_blockquote(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.indent_stack
            .push(IndentContext::new(vec![Span::from("> ")], None, false));
    }

    fn end_blockquote(&mut self) {
        self.indent_stack.pop();
        self.needs_newline = true;
    }

    fn text(&mut self, text: CowStr<'a>) {
        if let Some(buffer) = self.buffered_code_block.as_mut() {
            for line in text.lines() {
                buffer.lines.push(line.to_string());
            }
            // Code blocks always continue until TagEnd::CodeBlock.
            self.needs_newline = false;
            self.pending_marker_line = false;
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
        }
        self.pending_marker_line = false;
        if self.in_code_block && !self.needs_newline {
            let has_content = self
                .current_line_content
                .as_ref()
                .map(|line| !line.spans.is_empty())
                .unwrap_or_else(|| {
                    self.text
                        .lines
                        .last()
                        .map(|line| !line.spans.is_empty())
                        .unwrap_or(false)
                });
            if has_content {
                self.push_line(Line::default());
            }
        }
        for (i, line) in text.lines().enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if i > 0 {
                self.push_line(Line::default());
            }
            let content = line.to_string();
            let span = Span::styled(
                content,
                self.inline_styles.last().copied().unwrap_or_default(),
            );
            self.push_span(span);
        }
        self.needs_newline = false;
    }

    fn code(&mut self, code: CowStr<'a>) {
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        let span = Span::from(code.into_string()).style(self.styles.code);
        self.push_span(span);
    }

    fn html(&mut self, html: CowStr<'a>, inline: bool) {
        self.pending_marker_line = false;
        for (i, line) in html.lines().enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if i > 0 {
                self.push_line(Line::default());
            }
            let style = self.inline_styles.last().copied().unwrap_or_default();
            self.push_span(Span::styled(line.to_string(), style));
        }
        self.needs_newline = !inline;
    }

    fn hard_break(&mut self) {
        if let Some(buffer) = self.buffered_code_block.as_mut() {
            buffer.lines.push(String::new());
            return;
        }
        self.push_line(Line::default());
    }

    fn soft_break(&mut self) {
        if let Some(buffer) = self.buffered_code_block.as_mut() {
            buffer.lines.push(String::new());
            return;
        }
        self.push_line(Line::default());
    }

    fn start_list(&mut self, index: Option<u64>) {
        if self.list_indices.is_empty() && self.needs_newline {
            self.push_line(Line::default());
        }
        self.list_indices.push(index);
    }

    fn end_list(&mut self) {
        self.list_indices.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        self.pending_marker_line = true;
        let depth = self.list_indices.len();
        let is_ordered = self
            .list_indices
            .last()
            .map(Option::is_some)
            .unwrap_or(false);
        let width = depth * 4 - 3;
        let marker = if let Some(last_index) = self.list_indices.last_mut() {
            match last_index {
                None => Some(vec![Span::styled(
                    " ".repeat(width - 1) + "- ",
                    self.styles.unordered_list_marker,
                )]),
                Some(index) => {
                    *index += 1;
                    Some(vec![Span::styled(
                        format!("{:width$}. ", *index - 1),
                        self.styles.ordered_list_marker,
                    )])
                }
            }
        } else {
            None
        };
        let indent_prefix = if depth == 0 {
            Vec::new()
        } else {
            let indent_len = if is_ordered { width + 2 } else { width + 1 };
            vec![Span::from(" ".repeat(indent_len))]
        };
        self.indent_stack
            .push(IndentContext::new(indent_prefix, marker, true));
        self.needs_newline = false;
    }

    fn start_codeblock(&mut self, lang: Option<String>, indent: Option<Span<'static>>) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;
        self.buffered_code_block = lang
            .as_deref()
            .and_then(HighlightLanguage::from_fence_info)
            .map(|lang| BufferedCodeBlock {
                lang,
                lines: Vec::new(),
            });
        self.indent_stack.push(IndentContext::new(
            vec![indent.unwrap_or_default()],
            None,
            false,
        ));
        self.needs_newline = true;
    }

    fn end_codeblock(&mut self) {
        if let Some(buffer) = self.buffered_code_block.take() {
            let source = buffer.lines.join("\n");
            for line in crate::render::highlight::highlight_to_lines(buffer.lang, &source) {
                self.push_line(line);
            }
        }
        self.needs_newline = true;
        self.in_code_block = false;
        self.indent_stack.pop();
    }

    fn push_inline_style(&mut self, style: Style) {
        let current = self.inline_styles.last().copied().unwrap_or_default();
        let merged = current.patch(style);
        self.inline_styles.push(merged);
    }

    fn pop_inline_style(&mut self) {
        self.inline_styles.pop();
    }

    fn push_link(&mut self, dest_url: String) {
        self.link = Some(dest_url);
    }

    fn pop_link(&mut self) {
        if let Some(link) = self.link.take() {
            self.push_span(" (".into());
            self.push_span(Span::styled(link, self.styles.link));
            self.push_span(")".into());
        }
    }

    fn flush_current_line(&mut self) {
        if let Some(line) = self.current_line_content.take() {
            let style = self.current_line_style;
            let line_str = line_to_plain_string(&line);
            let no_wrap_table = is_box_table_line(&line_str);
            // NB we don't wrap code in code blocks, in order to preserve whitespace for copy/paste.
            if !self.current_line_in_code_block
                && !no_wrap_table
                && let Some(width) = self.wrap_width
            {
                let opts = RtOptions::new(width)
                    .initial_indent(self.current_initial_indent.clone().into())
                    .subsequent_indent(self.current_subsequent_indent.clone().into());
                for wrapped in word_wrap_line(&line, opts) {
                    let owned = line_to_static(&wrapped).style(style);
                    self.text.lines.push(owned);
                }
            } else {
                let mut spans = self.current_initial_indent.clone();
                let mut line = line;
                spans.append(&mut line.spans);
                self.text.lines.push(Line::from_iter(spans).style(style));
            }
            self.current_initial_indent.clear();
            self.current_subsequent_indent.clear();
            self.current_line_in_code_block = false;
        }
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.flush_current_line();
        let blockquote_active = self
            .indent_stack
            .iter()
            .any(|ctx| ctx.prefix.iter().any(|s| s.content.contains('>')));
        let style = if blockquote_active {
            self.styles.blockquote
        } else {
            line.style
        };
        let was_pending = self.pending_marker_line;

        self.current_initial_indent = self.prefix_spans(was_pending);
        self.current_subsequent_indent = self.prefix_spans(false);
        self.current_line_style = style;
        self.current_line_content = Some(line);
        self.current_line_in_code_block = self.in_code_block;

        self.pending_marker_line = false;
    }

    fn push_span(&mut self, span: Span<'static>) {
        if let Some(line) = self.current_line_content.as_mut() {
            line.push_span(span);
        } else {
            self.push_line(Line::from(vec![span]));
        }
    }

    fn push_blank_line(&mut self) {
        self.flush_current_line();
        if self.indent_stack.iter().all(|ctx| ctx.is_list) {
            self.text.lines.push(Line::default());
        } else {
            self.push_line(Line::default());
            self.flush_current_line();
        }
    }

    fn prefix_spans(&self, pending_marker_line: bool) -> Vec<Span<'static>> {
        let mut prefix: Vec<Span<'static>> = Vec::new();
        let last_marker_index = if pending_marker_line {
            self.indent_stack
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, ctx)| if ctx.marker.is_some() { Some(i) } else { None })
        } else {
            None
        };
        let last_list_index = self.indent_stack.iter().rposition(|ctx| ctx.is_list);

        for (i, ctx) in self.indent_stack.iter().enumerate() {
            if pending_marker_line {
                if Some(i) == last_marker_index
                    && let Some(marker) = &ctx.marker
                {
                    prefix.extend(marker.iter().cloned());
                    continue;
                }
                if ctx.is_list && last_marker_index.is_some_and(|idx| idx > i) {
                    continue;
                }
            } else if ctx.is_list && Some(i) != last_list_index {
                continue;
            }
            prefix.extend(ctx.prefix.iter().cloned());
        }

        prefix
    }
}

fn line_to_plain_string(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}

fn is_box_table_line(text: &str) -> bool {
    let trimmed = text.trim_start();
    matches!(trimmed.chars().next(), Some('┌' | '├' | '└' | '│'))
}

fn terminal_width_cols() -> Option<usize> {
    match crossterm::terminal::size() {
        Ok((cols, _rows)) => Some(cols as usize),
        Err(_) => None,
    }
}

#[cfg(test)]
mod markdown_render_tests {
    include!("markdown_render_tests.rs");
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::text::Text;

    fn lines_to_strings(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn wraps_plain_text_when_width_provided() {
        let markdown = "This is a simple sentence that should wrap.";
        let rendered = render_markdown_text_with_width(markdown, Some(16));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "This is a simple".to_string(),
                "sentence that".to_string(),
                "should wrap.".to_string(),
            ]
        );
    }

    #[test]
    fn wraps_list_items_preserving_indent() {
        let markdown = "- first second third fourth";
        let rendered = render_markdown_text_with_width(markdown, Some(14));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec!["- first second".to_string(), "  third fourth".to_string(),]
        );
    }

    #[test]
    fn wraps_nested_lists() {
        let markdown =
            "- outer item with several words to wrap\n  - inner item that also needs wrapping";
        let rendered = render_markdown_text_with_width(markdown, Some(20));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "- outer item with".to_string(),
                "  several words to".to_string(),
                "  wrap".to_string(),
                "    - inner item".to_string(),
                "      that also".to_string(),
                "      needs wrapping".to_string(),
            ]
        );
    }

    #[test]
    fn wraps_ordered_lists() {
        let markdown = "1. ordered item contains many words for wrapping";
        let rendered = render_markdown_text_with_width(markdown, Some(18));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "1. ordered item".to_string(),
                "   contains many".to_string(),
                "   words for".to_string(),
                "   wrapping".to_string(),
            ]
        );
    }

    #[test]
    fn wraps_blockquotes() {
        let markdown = "> block quote with content that should wrap nicely";
        let rendered = render_markdown_text_with_width(markdown, Some(22));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "> block quote with".to_string(),
                "> content that should".to_string(),
                "> wrap nicely".to_string(),
            ]
        );
    }

    #[test]
    fn wraps_blockquotes_inside_lists() {
        let markdown = "- list item\n  > block quote inside list that wraps";
        let rendered = render_markdown_text_with_width(markdown, Some(24));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "- list item".to_string(),
                "  > block quote inside".to_string(),
                "  > list that wraps".to_string(),
            ]
        );
    }

    #[test]
    fn wraps_list_items_containing_blockquotes() {
        let markdown = "1. item with quote\n   > quoted text that should wrap";
        let rendered = render_markdown_text_with_width(markdown, Some(24));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec![
                "1. item with quote".to_string(),
                "   > quoted text that".to_string(),
                "   > should wrap".to_string(),
            ]
        );
    }

    #[test]
    fn does_not_wrap_code_blocks() {
        let markdown = "````\nfn main() { println!(\"hi from a long line\"); }\n````";
        let rendered = render_markdown_text_with_width(markdown, Some(10));
        let lines = lines_to_strings(&rendered);
        assert_eq!(
            lines,
            vec!["fn main() { println!(\"hi from a long line\"); }".to_string(),]
        );
    }
}
