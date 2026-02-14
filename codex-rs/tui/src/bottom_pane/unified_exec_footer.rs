use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::live_wrap::take_prefix_by_width;
use crate::render::renderable::Renderable;

pub(crate) struct UnifiedExecFooter {
    processes: Vec<String>,
}

impl UnifiedExecFooter {
    pub(crate) fn new() -> Self {
        Self {
            processes: Vec::new(),
        }
    }

    pub(crate) fn set_processes(&mut self, processes: Vec<String>) -> bool {
        if self.processes == processes {
            return false;
        }
        self.processes = processes;
        true
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }

    fn process_snippet(process: &str) -> (String, bool) {
        match process.split_once('\n') {
            Some((first, _)) => (first.to_string(), true),
            None => (process.to_string(), false),
        }
    }

    fn truncate_snippet(snippet: &str, width: usize, has_hidden_content: bool) -> String {
        if width == 0 {
            return String::new();
        }

        let truncation_suffix = " [...]";
        let truncation_suffix_width = UnicodeWidthStr::width(truncation_suffix);
        let (truncated, remainder, _) = take_prefix_by_width(snippet, width);
        let needs_suffix = has_hidden_content || !remainder.is_empty();

        if needs_suffix && width > truncation_suffix_width {
            let available = width.saturating_sub(truncation_suffix_width);
            let (shorter, _, _) = take_prefix_by_width(snippet, available);
            format!("{shorter}{truncation_suffix}")
        } else {
            truncated
        }
    }

    fn render_message(&self, width: usize) -> Option<String> {
        if self.processes.is_empty() || width < 4 {
            return None;
        }

        let count = self.processes.len();
        let plural = if count == 1 { "" } else { "s" };
        let details_hint = " · /ps";
        let details_hint_width = UnicodeWidthStr::width(details_hint);

        let (snippet, snippet_has_hidden_content) = Self::process_snippet(&self.processes[0]);
        let more_suffix = if count > 1 {
            format!(" (+{} more)", count - 1)
        } else {
            String::new()
        };
        let prefix = format!("  {count} background terminal{plural} running: ");

        if width <= details_hint_width {
            let (minimal, _, _) = take_prefix_by_width(" /ps", width);
            return Some(minimal);
        }

        let body_width = width.saturating_sub(details_hint_width);
        let prefix_width = UnicodeWidthStr::width(prefix.as_str());
        let body = if prefix_width >= body_width {
            let (truncated, _, _) = take_prefix_by_width(&prefix, body_width);
            truncated
        } else {
            let remaining_width = body_width.saturating_sub(prefix_width);
            let include_more = if more_suffix.is_empty() {
                false
            } else {
                let more_width = UnicodeWidthStr::width(more_suffix.as_str());
                remaining_width > more_width.saturating_add(1)
            };
            let visible_more = if include_more {
                more_suffix.as_str()
            } else {
                ""
            };
            let more_width = UnicodeWidthStr::width(visible_more);
            let snippet_width = remaining_width.saturating_sub(more_width);
            let snippet =
                Self::truncate_snippet(&snippet, snippet_width, snippet_has_hidden_content);
            format!("{prefix}{snippet}{visible_more}")
        };
        Some(format!("{body}{details_hint}"))
    }

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(message) = self.render_message(width as usize) else {
            return Vec::new();
        };
        let (truncated, _, _) = take_prefix_by_width(&message, width as usize);
        vec![Line::from(truncated).dim()]
    }
}

impl Renderable for UnifiedExecFooter {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        Paragraph::new(self.render_lines(area.width)).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.render_lines(width).len() as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn render_text(footer: &UnifiedExecFooter, width: u16) -> String {
        footer
            .render_lines(width)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect::<String>()
    }

    #[test]
    fn desired_height_empty() {
        let footer = UnifiedExecFooter::new();
        assert_eq!(footer.desired_height(40), 0);
    }

    #[test]
    fn render_more_sessions() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec!["rg \"foo\" src".to_string()]);
        let width = 50;
        let height = footer.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        footer.render(Rect::new(0, 0, width, height), &mut buf);
        assert_snapshot!("render_more_sessions", format!("{buf:?}"));
    }

    #[test]
    fn render_many_sessions() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes((0..123).map(|idx| format!("cmd {idx}")).collect());
        let width = 50;
        let height = footer.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        footer.render(Rect::new(0, 0, width, height), &mut buf);
        assert_snapshot!("render_many_sessions", format!("{buf:?}"));
    }

    #[test]
    fn narrow_width_keeps_ps_hint() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec!["cargo test -p codex-core".to_string()]);
        let rendered = render_text(&footer, 10);
        assert!(
            rendered.contains("/ps"),
            "expected narrow footer to keep /ps hint, got: {rendered:?}"
        );
    }

    #[test]
    fn multiline_processes_show_truncated_snippet() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec!["echo hello\nand then continue".to_string()]);
        let rendered = render_text(&footer, 80);
        assert!(rendered.contains("echo hello"));
        assert!(rendered.contains("[...]"));
        assert!(!rendered.contains("and then continue"));
    }
}
