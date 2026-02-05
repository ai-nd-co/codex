use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter_highlight::Highlight;
use tree_sitter_highlight::HighlightConfiguration;
use tree_sitter_highlight::HighlightEvent;
use tree_sitter_highlight::Highlighter;

/// Languages supported by the tree-sitter highlighter in the TUI.
///
/// Notes:
/// - This is intentionally small to keep binary size reasonable.
/// - Add new languages only after confirming they ship usable highlight queries.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum HighlightLanguage {
    Bash,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Json,
    Toml,
    Yaml,
    Rust,
}

impl HighlightLanguage {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "sh" | "bash" => Some(Self::Bash),
            "py" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    pub(crate) fn from_fence_info(info: &str) -> Option<Self> {
        // "```ts" or "```typescript" or "```python"
        let raw = info.trim().split_whitespace().next().unwrap_or("");
        if raw.is_empty() {
            return None;
        }
        let normalized = raw.to_ascii_lowercase();
        match normalized.as_str() {
            "bash" | "sh" => Some(Self::Bash),
            "py" | "python" => Some(Self::Python),
            "js" | "javascript" => Some(Self::JavaScript),
            "ts" | "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "rs" | "rust" => Some(Self::Rust),
            _ => None,
        }
    }
}

/// Capture names used by tree-sitter highlight queries across many languages.
///
/// We configure each `HighlightConfiguration` with this fixed list and then map
/// capture names to ratatui styles.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "boolean",
    "character",
    "comment",
    "constant",
    "constructor",
    "embedded",
    "error",
    "escape",
    "function",
    "function.builtin",
    "keyword",
    "label",
    "module",
    "namespace",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "tag",
    "tag.builtin",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

fn config_bash() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_bash::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        )
        .expect("load bash highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_python() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_python::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load python highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_javascript() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_javascript::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            "",
            "",
        )
        .expect("load javascript highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_typescript() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "typescript",
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load typescript highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_tsx() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_typescript::LANGUAGE_TSX.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "tsx",
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load tsx highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_json() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_json::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load json highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_toml() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_toml_ng::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "toml",
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load toml highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_yaml() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_yaml::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load yaml highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_rust() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_rust::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load rust highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn highlight_name_for(highlight: Highlight) -> &'static str {
    HIGHLIGHT_NAMES
        .get(highlight.0)
        .copied()
        .unwrap_or("unknown")
}

fn style_for_capture(lang: HighlightLanguage, capture: &str) -> Style {
    // Keep bash highlighting conservative to preserve existing UI + tests:
    // bash dims operators/strings/comments but does not apply a full theme.
    if lang == HighlightLanguage::Bash {
        return match capture {
            "comment" | "operator" | "string" => Style::default().dim(),
            _ => Style::default(),
        };
    }

    match capture {
        // Darcula-ish palette (JetBrains default dark).
        //
        // NOTE: terminals don't support opacity; we only set foreground colors here.
        // We intentionally use RGB colors directly (instead of best_color()) to avoid
        // terminal palette detection issues causing "everything looks gray".
        "comment" => Style::default().fg(darcula_rgb(128, 128, 128)).dim(),
        "string" | "string.special" | "character" | "escape" => {
            Style::default().fg(darcula_rgb(106, 135, 89))
        }
        "number" | "boolean" => Style::default().fg(darcula_rgb(104, 151, 187)),
        "keyword" => Style::default().fg(darcula_rgb(204, 120, 50)).bold(),
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            Style::default().fg(darcula_rgb(169, 183, 198)).dim()
        }
        "function" | "function.builtin" | "constructor" => {
            Style::default().fg(darcula_rgb(255, 198, 109))
        }
        "type" | "type.builtin" => Style::default().fg(darcula_rgb(152, 118, 170)),
        "constant" => Style::default().fg(darcula_rgb(152, 118, 170)),
        "variable" | "variable.parameter" | "variable.builtin" => {
            Style::default().fg(darcula_rgb(169, 183, 198))
        }
        "property" | "attribute" => Style::default().fg(darcula_rgb(187, 181, 41)),
        "tag" | "tag.builtin" => Style::default().fg(darcula_rgb(204, 120, 50)),
        "module" | "namespace" => Style::default().fg(darcula_rgb(169, 183, 198)),
        "label" => Style::default().fg(darcula_rgb(152, 118, 170)),
        "embedded" => Style::default().fg(darcula_rgb(106, 135, 89)),
        "error" => Style::default().fg(darcula_rgb(255, 85, 85)).bold(),
        _ => Style::default(),
    }
}

fn darcula_rgb(r: u8, g: u8, b: u8) -> Color {
    #[allow(clippy::disallowed_methods)]
    Color::Rgb(r, g, b)
}

fn highlight_config(lang: HighlightLanguage) -> &'static HighlightConfiguration {
    match lang {
        HighlightLanguage::Bash => config_bash(),
        HighlightLanguage::Python => config_python(),
        HighlightLanguage::JavaScript => config_javascript(),
        HighlightLanguage::TypeScript => config_typescript(),
        HighlightLanguage::Tsx => config_tsx(),
        HighlightLanguage::Json => config_json(),
        HighlightLanguage::Toml => config_toml(),
        HighlightLanguage::Yaml => config_yaml(),
        HighlightLanguage::Rust => config_rust(),
    }
}

fn push_segment(lines: &mut Vec<Line<'static>>, segment: &str, style: Option<Style>) {
    for (i, part) in segment.split('\n').enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        if part.is_empty() {
            continue;
        }
        let span = match style {
            Some(style) => Span::styled(part.to_string(), style),
            None => part.to_string().into(),
        };
        if let Some(last) = lines.last_mut() {
            last.spans.push(span);
        }
    }
}

/// Convert a bash script into per-line styled content using tree-sitter's
/// bash highlight query. The highlighter is streamed so multi-line content is
/// split into `Line`s while preserving style boundaries.
pub(crate) fn highlight_bash_to_lines(script: &str) -> Vec<Line<'static>> {
    highlight_to_lines(HighlightLanguage::Bash, script)
}

pub(crate) fn highlight_to_lines(lang: HighlightLanguage, source: &str) -> Vec<Line<'static>> {
    let mut highlighter = Highlighter::new();
    let iterator =
        match highlighter.highlight(highlight_config(lang), source.as_bytes(), None, |_| None) {
            Ok(iter) => iter,
            Err(_) => return vec![source.to_string().into()],
        };

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    let mut highlight_stack: Vec<Highlight> = Vec::new();

    for event in iterator {
        match event {
            Ok(HighlightEvent::HighlightStart(highlight)) => highlight_stack.push(highlight),
            Ok(HighlightEvent::HighlightEnd) => {
                highlight_stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if start == end {
                    continue;
                }
                let style = highlight_stack.last().map(|h| {
                    let name = highlight_name_for(*h);
                    style_for_capture(lang, name)
                });
                push_segment(&mut lines, &source[start..end], style);
            }
            Err(_) => return vec![source.to_string().into()],
        }
    }

    if lines.is_empty() {
        vec![Line::from("")]
    } else {
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;

    fn reconstructed(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|sp| sp.content.clone())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn dimmed_tokens(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|sp| sp.style.add_modifier.contains(Modifier::DIM))
            .map(|sp| sp.content.clone().into_owned())
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
            .collect()
    }

    #[test]
    fn dims_expected_bash_operators() {
        let s = "echo foo && bar || baz | qux & (echo hi)";
        let lines = highlight_bash_to_lines(s);
        assert_eq!(reconstructed(&lines), s);

        let dimmed = dimmed_tokens(&lines);
        assert!(dimmed.contains(&"&&".to_string()));
        assert!(dimmed.contains(&"|".to_string()));
        assert!(!dimmed.contains(&"echo".to_string()));
    }

    #[test]
    fn dims_redirects_and_strings() {
        let s = "echo \"hi\" > out.txt; echo 'ok'";
        let lines = highlight_bash_to_lines(s);
        assert_eq!(reconstructed(&lines), s);

        let dimmed = dimmed_tokens(&lines);
        assert!(dimmed.contains(&">".to_string()));
        assert!(dimmed.contains(&"\"hi\"".to_string()));
        assert!(dimmed.contains(&"'ok'".to_string()));
    }

    #[test]
    fn highlights_command_and_strings() {
        let s = "echo \"hi\"";
        let lines = highlight_bash_to_lines(s);
        let mut echo_style = None;
        let mut string_style = None;
        for span in &lines[0].spans {
            let text = span.content.as_ref();
            if text == "echo" {
                echo_style = Some(span.style);
            }
            if text == "\"hi\"" {
                string_style = Some(span.style);
            }
        }
        let echo_style = echo_style.expect("echo span missing");
        let string_style = string_style.expect("string span missing");
        assert!(echo_style.fg.is_none());
        assert!(!echo_style.add_modifier.contains(Modifier::DIM));
        assert!(string_style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn highlights_heredoc_body_as_string() {
        let s = "cat <<EOF\nheredoc body\nEOF";
        let lines = highlight_bash_to_lines(s);
        let body_line = &lines[1];
        let mut body_style = None;
        for span in &body_line.spans {
            if span.content.as_ref() == "heredoc body" {
                body_style = Some(span.style);
            }
        }
        let body_style = body_style.expect("missing heredoc span");
        assert!(body_style.add_modifier.contains(Modifier::DIM));
    }
}
