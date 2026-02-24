use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::live_wrap::take_prefix_by_width;
use crate::render::renderable::Renderable;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnifiedExecProcessDetails {
    pub(crate) command_display: String,
    pub(crate) recent_chunks: Vec<String>,
}

pub(crate) struct UnifiedExecFooter {
    processes: Vec<UnifiedExecProcessDetails>,
}

impl UnifiedExecFooter {
    pub(crate) fn new() -> Self {
        Self {
            processes: Vec::default(),
        }
    }

    pub(crate) fn set_processes(&mut self, processes: Vec<UnifiedExecProcessDetails>) -> bool {
        if self.processes == processes {
            return false;
        }
        self.processes = processes;
        true
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }

    fn process_snippet(command: &str) -> (String, bool) {
        match command.split_once('\n') {
            Some((first, _)) => (first.to_string(), true),
            None => (command.to_string(), false),
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

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.processes.is_empty() || width == 0 {
            return Vec::new();
        }

        let wrap_width = width as usize;
        let mut out: Vec<Line<'static>> = Vec::new();
        let count = self.processes.len();
        let plural = if count == 1 { "" } else { "s" };
        let header = format!("  {count} background terminal{plural} running");
        let (header, _, _) = take_prefix_by_width(&header, wrap_width);
        out.push(Line::from(header).dim());

        let max_processes = 16usize;
        let prefix = "  • ";
        let prefix_width = UnicodeWidthStr::width(prefix);
        let truncation_suffix = " [...]";
        let truncation_suffix_width = UnicodeWidthStr::width(truncation_suffix);

        let mut shown = 0usize;
        for process in &self.processes {
            if shown >= max_processes {
                break;
            }

            let (snippet, snippet_has_hidden_content) =
                Self::process_snippet(&process.command_display);
            if wrap_width <= prefix_width {
                out.push(Line::from(prefix.dim()));
                shown += 1;
                continue;
            }
            let budget = wrap_width.saturating_sub(prefix_width);
            let snippet = Self::truncate_snippet(&snippet, budget, snippet_has_hidden_content);
            if snippet.ends_with(truncation_suffix) && budget > truncation_suffix_width {
                let visible = snippet.trim_end_matches(truncation_suffix).to_string();
                out.push(vec![prefix.dim(), visible.cyan(), truncation_suffix.dim()].into());
            } else {
                out.push(vec![prefix.dim(), snippet.cyan()].into());
            }

            let chunk_prefix_first = "    ↳ ";
            let chunk_prefix_next = "      ";
            for (idx, chunk) in process.recent_chunks.iter().enumerate() {
                let chunk_prefix = if idx == 0 {
                    chunk_prefix_first
                } else {
                    chunk_prefix_next
                };
                let chunk_prefix_width = UnicodeWidthStr::width(chunk_prefix);
                if wrap_width <= chunk_prefix_width {
                    out.push(Line::from(chunk_prefix.dim()));
                    continue;
                }
                let budget = wrap_width.saturating_sub(chunk_prefix_width);
                let (truncated, remainder, _) = take_prefix_by_width(chunk, budget);
                if !remainder.is_empty() && budget > truncation_suffix_width {
                    let available = budget.saturating_sub(truncation_suffix_width);
                    let (shorter, _, _) = take_prefix_by_width(chunk, available);
                    out.push(
                        vec![chunk_prefix.dim(), shorter.dim(), truncation_suffix.dim()].into(),
                    );
                } else {
                    out.push(vec![chunk_prefix.dim(), truncated.dim()].into());
                }
            }

            shown += 1;
        }

        let remaining = self.processes.len().saturating_sub(shown);
        if remaining > 0 {
            let more_text = format!("... and {remaining} more running");
            if wrap_width <= prefix_width {
                out.push(Line::from(prefix.dim()));
            } else {
                let budget = wrap_width.saturating_sub(prefix_width);
                let (truncated, _, _) = take_prefix_by_width(&more_text, budget);
                out.push(vec![prefix.dim(), truncated.dim()].into());
            }
        }

        out
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
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn desired_height_empty() {
        let footer = UnifiedExecFooter::new();
        assert_eq!(footer.desired_height(40), 0);
    }

    #[test]
    fn render_process_details_with_chunks() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec![
            UnifiedExecProcessDetails {
                command_display: "cargo test -p codex-core".to_string(),
                recent_chunks: vec!["Compiling codex-core".to_string()],
            },
            UnifiedExecProcessDetails {
                command_display: "rg \"foo\" src".to_string(),
                recent_chunks: vec!["src/main.rs:12:foo".to_string()],
            },
        ]);
        let width = 50;
        let height = footer.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        footer.render(Rect::new(0, 0, width, height), &mut buf);
        assert_snapshot!("render_process_details_with_chunks", format!("{buf:?}"));
    }

    #[test]
    fn render_many_sessions() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(
            (0..123)
                .map(|idx| UnifiedExecProcessDetails {
                    command_display: format!("cmd {idx}"),
                    recent_chunks: Vec::new(),
                })
                .collect(),
        );
        let width = 50;
        let height = footer.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        footer.render(Rect::new(0, 0, width, height), &mut buf);
        assert_snapshot!("render_many_sessions", format!("{buf:?}"));
    }

    #[test]
    fn narrow_width_does_not_reference_ps() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec![UnifiedExecProcessDetails {
            command_display: "cargo test -p codex-core".to_string(),
            recent_chunks: Vec::new(),
        }]);
        let rendered = render_text(&footer, 10);
        assert!(
            !rendered.contains("/ps"),
            "expected footer to avoid /ps hint, got: {rendered:?}"
        );
    }

    #[test]
    fn multiline_processes_show_truncated_snippet() {
        let mut footer = UnifiedExecFooter::new();
        footer.set_processes(vec![UnifiedExecProcessDetails {
            command_display: "echo hello\nand then continue".to_string(),
            recent_chunks: Vec::new(),
        }]);
        let rendered = render_text(&footer, 80);
        assert!(rendered.contains("echo hello"));
        assert!(rendered.contains("[...]"));
        assert!(!rendered.contains("and then continue"));
    }
}
