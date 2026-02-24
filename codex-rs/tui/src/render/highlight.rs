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
    Css,
    Html,
    Sql,
    Java,
    Kotlin,
    Dart,
    Hcl,
    Markdown,
    Xml,
    Dockerfile,
    Dotenv,
    Ini,
}

impl HighlightLanguage {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let file_name = path.file_name().and_then(|name| name.to_str())?;
        let file_name = file_name.to_ascii_lowercase();
        if file_name == "dockerfile" || file_name.starts_with("dockerfile.") {
            return Some(Self::Dockerfile);
        }
        if file_name == ".env" || file_name.starts_with(".env.") {
            return Some(Self::Dotenv);
        }

        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "sh" | "bash" => Some(Self::Bash),
            "py" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "json" => Some(Self::Json),
            "md" => Some(Self::Markdown),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "rs" => Some(Self::Rust),
            "css" => Some(Self::Css),
            // Tree-sitter scss doesn't currently build cleanly on Windows (MSVC)
            // in this workspace; fall back to CSS highlighting.
            "scss" => Some(Self::Css),
            "html" | "htm" => Some(Self::Html),
            "xml" | "svg" => Some(Self::Xml),
            "ini" => Some(Self::Ini),
            "sql" => Some(Self::Sql),
            "java" => Some(Self::Java),
            "kt" | "kts" => Some(Self::Kotlin),
            "dart" => Some(Self::Dart),
            "tf" | "hcl" => Some(Self::Hcl),
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
            "md" | "markdown" => Some(Self::Markdown),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "rs" | "rust" => Some(Self::Rust),
            "css" => Some(Self::Css),
            "scss" => Some(Self::Css),
            "html" => Some(Self::Html),
            "xml" => Some(Self::Xml),
            "sql" => Some(Self::Sql),
            "java" => Some(Self::Java),
            "kotlin" | "kt" => Some(Self::Kotlin),
            "dart" | "flutter" => Some(Self::Dart),
            "hcl" | "terraform" => Some(Self::Hcl),
            "dockerfile" => Some(Self::Dockerfile),
            "dotenv" | "env" => Some(Self::Dotenv),
            "ini" => Some(Self::Ini),
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
    "constant.builtin",
    "constructor",
    "conditional",
    "embedded",
    "error",
    "escape",
    "exception",
    "function",
    "function.builtin",
    "include",
    "keyword",
    "label",
    "method",
    "module",
    "namespace",
    "number",
    "operator",
    "parameter",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "repeat",
    "string",
    "string.special",
    "string.escape",
    "symbol",
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

fn config_css() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_css::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "css", tree_sitter_css::HIGHLIGHTS_QUERY, "", "")
                .expect("load css highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_html() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_html::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        )
        .expect("load html highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_sql() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_sequel::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "sql",
            tree_sitter_sequel::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load sql highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_java() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_java::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load java highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

fn config_kotlin() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_kotlin_codanna::language();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(
            language,
            "kotlin",
            tree_sitter_kotlin_codanna::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .expect("load kotlin highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

const DART_HIGHLIGHTS_QUERY: &str = include_str!("queries/dart_highlights.scm");
fn config_dart() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_dart::language();
        #[expect(clippy::expect_used)]
        let mut config =
            HighlightConfiguration::new(language, "dart", DART_HIGHLIGHTS_QUERY, "", "")
                .expect("load dart highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

const HCL_HIGHLIGHTS_QUERY: &str = include_str!("queries/hcl_highlights.scm");
fn config_hcl() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_hcl::LANGUAGE.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(language, "hcl", HCL_HIGHLIGHTS_QUERY, "", "")
            .expect("load hcl highlight query");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

const XML_HIGHLIGHTS_QUERY: &str = include_str!("queries/xml_highlights.scm");
fn config_xml() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = tree_sitter_xml::LANGUAGE_XML.into();
        #[expect(clippy::expect_used)]
        let mut config = HighlightConfiguration::new(language, "xml", XML_HIGHLIGHTS_QUERY, "", "")
            .expect("load xml highlight query");
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
        "string" | "string.special" | "string.escape" | "character" | "escape" => {
            Style::default().fg(darcula_rgb(106, 135, 89))
        }
        "number" | "boolean" => Style::default().fg(darcula_rgb(104, 151, 187)),
        "keyword" | "include" | "conditional" | "exception" | "repeat" => {
            Style::default().fg(darcula_rgb(204, 120, 50)).bold()
        }
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            Style::default().fg(darcula_rgb(169, 183, 198)).dim()
        }
        "punctuation.special" => Style::default().fg(darcula_rgb(169, 183, 198)).dim(),
        "function" | "function.builtin" | "constructor" | "method" => {
            Style::default().fg(darcula_rgb(255, 198, 109))
        }
        "type" | "type.builtin" => Style::default().fg(darcula_rgb(152, 118, 170)),
        "constant" | "constant.builtin" | "symbol" => {
            Style::default().fg(darcula_rgb(152, 118, 170))
        }
        "variable" | "variable.parameter" | "variable.builtin" | "parameter" => {
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
        HighlightLanguage::Css => config_css(),
        HighlightLanguage::Html => config_html(),
        HighlightLanguage::Xml => config_xml(),
        HighlightLanguage::Sql => config_sql(),
        HighlightLanguage::Java => config_java(),
        HighlightLanguage::Kotlin => config_kotlin(),
        HighlightLanguage::Dart => config_dart(),
        HighlightLanguage::Hcl => config_hcl(),
        // Markdown/Dockerfile/Dotenv/INI are handled by lightweight tokenizers (see highlight_to_lines).
        // These match arms should never be hit, but must exist for exhaustiveness.
        HighlightLanguage::Markdown
        | HighlightLanguage::Dockerfile
        | HighlightLanguage::Dotenv
        | HighlightLanguage::Ini => config_bash(),
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
    match lang {
        HighlightLanguage::Markdown => return highlight_markdown_to_lines(source),
        HighlightLanguage::Dockerfile => return highlight_dockerfile_to_lines(source),
        HighlightLanguage::Dotenv => return highlight_dotenv_to_lines(source),
        HighlightLanguage::Ini => return highlight_ini_to_lines(source),
        _ => {}
    }

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

fn highlight_markdown_to_lines(source: &str) -> Vec<Line<'static>> {
    let heading_style = style_for_capture(HighlightLanguage::Markdown, "keyword");
    let list_marker_style = style_for_capture(HighlightLanguage::Markdown, "punctuation.special");
    let backtick_style = style_for_capture(HighlightLanguage::Markdown, "punctuation.special");
    let code_style = style_for_capture(HighlightLanguage::Markdown, "string");

    fn push_inline_with_code(
        line: &mut Line<'static>,
        s: &str,
        base_style: Option<Style>,
        backtick_style: Style,
        code_style: Style,
    ) {
        let mut buf = String::new();
        let mut in_code = false;
        for ch in s.chars() {
            if ch == '`' {
                if !buf.is_empty() {
                    let style = if in_code {
                        Some(code_style)
                    } else {
                        base_style
                    };
                    line.spans.push(match style {
                        Some(style) => Span::styled(std::mem::take(&mut buf), style),
                        None => std::mem::take(&mut buf).into(),
                    });
                }
                line.spans
                    .push(Span::styled("`".to_string(), backtick_style));
                in_code = !in_code;
                continue;
            }
            buf.push(ch);
        }
        if !buf.is_empty() {
            let style = if in_code {
                Some(code_style)
            } else {
                base_style
            };
            line.spans.push(match style {
                Some(style) => Span::styled(buf, style),
                None => buf.into(),
            });
        }
    }

    let mut out = Vec::new();
    for raw in source.split('\n') {
        let mut line = Line::from("");
        if raw.is_empty() {
            out.push(line);
            continue;
        }

        let trimmed = raw.trim_start();
        let indent_len = raw.len() - trimmed.len();
        if indent_len > 0 {
            line.spans.push(raw[..indent_len].to_string().into());
        }

        // Fences: ```lang
        if trimmed.starts_with("```") {
            line.spans
                .push(Span::styled(trimmed.to_string(), list_marker_style));
            out.push(line);
            continue;
        }

        // Heading: # ... (only treat as heading when there is at least one space after hashes)
        let hash_count = trimmed.chars().take_while(|c| *c == '#').count();
        if hash_count > 0 {
            let after = &trimmed[hash_count..];
            if after.starts_with(' ') || after.starts_with('\t') {
                line.spans
                    .push(Span::styled("#".repeat(hash_count), list_marker_style));
                push_inline_with_code(
                    &mut line,
                    after,
                    Some(heading_style),
                    backtick_style,
                    code_style,
                );
                out.push(line);
                continue;
            }
        }

        // List marker (common)
        let is_bullet =
            trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ");
        let is_ordered = trimmed.chars().take_while(|c| c.is_ascii_digit()).count() > 0
            && (trimmed.contains(". ") || trimmed.contains(") "));
        if is_bullet {
            line.spans
                .push(Span::styled(trimmed[..1].to_string(), list_marker_style));
            push_inline_with_code(&mut line, &trimmed[1..], None, backtick_style, code_style);
            out.push(line);
            continue;
        }
        if is_ordered {
            // Highlight leading "<digits>." or "<digits>)" marker.
            let mut marker_end = 0usize;
            for (i, ch) in trimmed.char_indices() {
                marker_end = i + ch.len_utf8();
                if ch == '.' || ch == ')' {
                    break;
                }
                if !ch.is_ascii_digit() {
                    marker_end = 0;
                    break;
                }
            }
            if marker_end > 0 && trimmed[marker_end..].starts_with(' ') {
                line.spans.push(Span::styled(
                    trimmed[..marker_end].to_string(),
                    list_marker_style,
                ));
                push_inline_with_code(
                    &mut line,
                    &trimmed[marker_end..],
                    None,
                    backtick_style,
                    code_style,
                );
                out.push(line);
                continue;
            }
        }

        // Default: just inline-code highlighting.
        push_inline_with_code(&mut line, trimmed, None, backtick_style, code_style);
        out.push(line);
    }
    out
}

fn highlight_dockerfile_to_lines(source: &str) -> Vec<Line<'static>> {
    let comment_style = style_for_capture(HighlightLanguage::Dockerfile, "comment");
    let keyword_style = style_for_capture(HighlightLanguage::Dockerfile, "keyword");
    let op_style = style_for_capture(HighlightLanguage::Dockerfile, "operator");
    let string_style = style_for_capture(HighlightLanguage::Dockerfile, "string");
    let var_style = style_for_capture(HighlightLanguage::Dockerfile, "variable");

    // Keep this list aligned with common Dockerfile instructions.
    const KEYWORDS: &[&str] = &[
        "FROM",
        "AS",
        "RUN",
        "CMD",
        "LABEL",
        "EXPOSE",
        "ENV",
        "ADD",
        "COPY",
        "ENTRYPOINT",
        "VOLUME",
        "USER",
        "WORKDIR",
        "ARG",
        "ONBUILD",
        "STOPSIGNAL",
        "HEALTHCHECK",
        "SHELL",
        "MAINTAINER",
    ];

    fn is_keyword(tok: &str) -> bool {
        KEYWORDS.iter().any(|k| tok.eq_ignore_ascii_case(k))
    }

    let mut out = Vec::new();
    for raw in source.split('\n') {
        let mut line = Line::from("");
        if raw.is_empty() {
            out.push(line);
            continue;
        }
        let trimmed = raw.trim_start();
        let indent_len = raw.len() - trimmed.len();
        if indent_len > 0 {
            line.spans.push(raw[..indent_len].to_string().into());
        }
        if trimmed.starts_with('#') {
            line.spans
                .push(Span::styled(trimmed.to_string(), comment_style));
            out.push(line);
            continue;
        }

        // Highlight first token if it matches a Dockerfile instruction.
        let mut rest = trimmed;
        let first_ws = trimmed
            .char_indices()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx));
        if let Some(ws_idx) = first_ws {
            let tok = &trimmed[..ws_idx];
            if is_keyword(tok) {
                line.spans
                    .push(Span::styled(tok.to_string(), keyword_style));
                rest = &trimmed[ws_idx..];
            }
        } else if is_keyword(trimmed) {
            line.spans
                .push(Span::styled(trimmed.to_string(), keyword_style));
            out.push(line);
            continue;
        }

        // Highlight simple variable expansions ($VAR / ${VAR}) and quoted strings.
        let mut buf = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '$' {
                if !buf.is_empty() {
                    line.spans.push(std::mem::take(&mut buf).into());
                }
                line.spans.push(Span::styled("$".to_string(), op_style));
                if chars.peek() == Some(&'{') {
                    chars.next();
                    line.spans.push(Span::styled("{".to_string(), op_style));
                    let mut name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == '}' {
                            break;
                        }
                        name.push(c);
                        chars.next();
                    }
                    if !name.is_empty() {
                        line.spans.push(Span::styled(name, var_style));
                    }
                    if chars.peek() == Some(&'}') {
                        chars.next();
                        line.spans.push(Span::styled("}".to_string(), op_style));
                    }
                    continue;
                }
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if !(c.is_ascii_alphanumeric() || c == '_') {
                        break;
                    }
                    name.push(c);
                    chars.next();
                }
                if !name.is_empty() {
                    line.spans.push(Span::styled(name, var_style));
                }
                continue;
            }
            if ch == '"' || ch == '\'' {
                if !buf.is_empty() {
                    line.spans.push(std::mem::take(&mut buf).into());
                }
                let quote = ch;
                let mut s = String::new();
                s.push(quote);
                while let Some(c) = chars.next() {
                    s.push(c);
                    if c == quote {
                        break;
                    }
                }
                line.spans.push(Span::styled(s, string_style));
                continue;
            }
            buf.push(ch);
        }
        if !buf.is_empty() {
            line.spans.push(buf.into());
        }
        out.push(line);
    }
    out
}

fn highlight_dotenv_to_lines(source: &str) -> Vec<Line<'static>> {
    let comment_style = style_for_capture(HighlightLanguage::Dotenv, "comment");
    let key_style = style_for_capture(HighlightLanguage::Dotenv, "attribute");
    let op_style = style_for_capture(HighlightLanguage::Dotenv, "operator");
    let string_style = style_for_capture(HighlightLanguage::Dotenv, "string");
    let number_style = style_for_capture(HighlightLanguage::Dotenv, "number");

    let mut out = Vec::new();
    for line in source.split('\n') {
        let mut rendered = Line::from("");
        if line.is_empty() {
            out.push(rendered);
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            rendered
                .spans
                .push(Span::styled(line.to_string(), comment_style));
            out.push(rendered);
            continue;
        }

        // Parse: optional "export", KEY, '=', VALUE
        let idx = if trimmed.starts_with("export ") {
            let export_prefix_len = line.len() - trimmed.len() + "export ".len();
            rendered.spans.push(Span::styled(
                line[..export_prefix_len].to_string(),
                style_for_capture(HighlightLanguage::Dotenv, "keyword"),
            ));
            export_prefix_len
        } else {
            line.len() - trimmed.len()
        };

        // Find '=' (first occurrence) after idx
        let Some(eq_pos) = line[idx..].find('=') else {
            rendered.spans.push(line.to_string().into());
            out.push(rendered);
            continue;
        };
        let eq_pos = idx + eq_pos;

        // Key segment (including whitespace before '=')
        let key_segment = &line[..eq_pos];
        // Attempt to find key token end (strip trailing whitespace)
        let key_token = key_segment.trim_end();
        let key_ws = &key_segment[key_token.len()..];
        if !key_token.is_empty() {
            rendered
                .spans
                .push(Span::styled(key_token.to_string(), key_style));
        }
        if !key_ws.is_empty() {
            rendered.spans.push(key_ws.to_string().into());
        }

        // '='
        rendered.spans.push(Span::styled("=".to_string(), op_style));

        // Value (preserve leading spaces)
        let value = &line[eq_pos + 1..];
        if value.is_empty() {
            out.push(rendered);
            continue;
        }
        let value_trimmed = value.trim_start();
        let leading = &value[..value.len() - value_trimmed.len()];
        if !leading.is_empty() {
            rendered.spans.push(leading.to_string().into());
        }

        let value_style = if value_trimmed.starts_with('"')
            || value_trimmed.starts_with('\'')
            || value_trimmed.contains('/')
            || value_trimmed.contains('.')
        {
            string_style
        } else if value_trimmed.chars().all(|c| c.is_ascii_digit()) {
            number_style
        } else {
            string_style
        };
        rendered
            .spans
            .push(Span::styled(value_trimmed.to_string(), value_style));
        out.push(rendered);
    }
    out
}

fn highlight_ini_to_lines(source: &str) -> Vec<Line<'static>> {
    let comment_style = style_for_capture(HighlightLanguage::Ini, "comment");
    let section_style = style_for_capture(HighlightLanguage::Ini, "keyword");
    let key_style = style_for_capture(HighlightLanguage::Ini, "attribute");
    let op_style = style_for_capture(HighlightLanguage::Ini, "operator");
    let string_style = style_for_capture(HighlightLanguage::Ini, "string");

    let mut out = Vec::new();
    for line in source.split('\n') {
        let mut rendered = Line::from("");
        if line.is_empty() {
            out.push(rendered);
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            rendered
                .spans
                .push(Span::styled(line.to_string(), comment_style));
            out.push(rendered);
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
            rendered
                .spans
                .push(Span::styled(line.to_string(), section_style));
            out.push(rendered);
            continue;
        }
        // key=value or key: value
        let Some((sep_idx, sep_ch)) = trimmed
            .char_indices()
            .find_map(|(i, ch)| (ch == '=' || ch == ':').then_some((i, ch)))
        else {
            rendered.spans.push(line.to_string().into());
            out.push(rendered);
            continue;
        };
        let leading_ws_len = line.len() - trimmed.len();
        if leading_ws_len > 0 {
            rendered
                .spans
                .push(line[..leading_ws_len].to_string().into());
        }
        let key_part = &trimmed[..sep_idx];
        let key_token = key_part.trim_end();
        let key_ws = &key_part[key_token.len()..];
        if !key_token.is_empty() {
            rendered
                .spans
                .push(Span::styled(key_token.to_string(), key_style));
        }
        if !key_ws.is_empty() {
            rendered.spans.push(key_ws.to_string().into());
        }
        rendered
            .spans
            .push(Span::styled(sep_ch.to_string(), op_style));
        let value = &trimmed[sep_idx + 1..];
        if !value.is_empty() {
            rendered
                .spans
                .push(Span::styled(value.to_string(), string_style));
        }
        out.push(rendered);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;
    use std::path::PathBuf;

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

    #[test]
    fn detects_languages_from_paths() {
        let cases = [
            ("foo.py", HighlightLanguage::Python),
            ("foo.ts", HighlightLanguage::TypeScript),
            ("foo.tsx", HighlightLanguage::Tsx),
            ("foo.json", HighlightLanguage::Json),
            ("README.md", HighlightLanguage::Markdown),
            ("foo.toml", HighlightLanguage::Toml),
            ("foo.yml", HighlightLanguage::Yaml),
            ("foo.rs", HighlightLanguage::Rust),
            ("foo.dart", HighlightLanguage::Dart),
            ("foo.sql", HighlightLanguage::Sql),
            ("foo.java", HighlightLanguage::Java),
            ("foo.kt", HighlightLanguage::Kotlin),
            ("foo.css", HighlightLanguage::Css),
            ("foo.scss", HighlightLanguage::Css),
            ("foo.html", HighlightLanguage::Html),
            ("foo.xml", HighlightLanguage::Xml),
            ("icon.svg", HighlightLanguage::Xml),
            ("Dockerfile", HighlightLanguage::Dockerfile),
            ("Dockerfile.dev", HighlightLanguage::Dockerfile),
            (".env", HighlightLanguage::Dotenv),
            (".env.local", HighlightLanguage::Dotenv),
            ("settings.ini", HighlightLanguage::Ini),
            ("main.tf", HighlightLanguage::Hcl),
        ];
        for (path, expected) in cases {
            let lang = HighlightLanguage::from_path(&PathBuf::from(path)).expect("language");
            assert_eq!(lang, expected, "path: {path}");
        }
    }

    #[test]
    fn detects_languages_from_fences() {
        let cases = [
            ("python", HighlightLanguage::Python),
            ("ts", HighlightLanguage::TypeScript),
            ("tsx", HighlightLanguage::Tsx),
            ("json", HighlightLanguage::Json),
            ("markdown", HighlightLanguage::Markdown),
            ("md", HighlightLanguage::Markdown),
            ("toml", HighlightLanguage::Toml),
            ("yaml", HighlightLanguage::Yaml),
            ("rust", HighlightLanguage::Rust),
            ("dart", HighlightLanguage::Dart),
            ("flutter", HighlightLanguage::Dart),
            ("xml", HighlightLanguage::Xml),
            ("sql", HighlightLanguage::Sql),
            ("java", HighlightLanguage::Java),
            ("kotlin", HighlightLanguage::Kotlin),
            ("css", HighlightLanguage::Css),
            ("scss", HighlightLanguage::Css),
            ("html", HighlightLanguage::Html),
            ("terraform", HighlightLanguage::Hcl),
            ("dockerfile", HighlightLanguage::Dockerfile),
            ("dotenv", HighlightLanguage::Dotenv),
            ("env", HighlightLanguage::Dotenv),
            ("ini", HighlightLanguage::Ini),
        ];
        for (fence, expected) in cases {
            let lang = HighlightLanguage::from_fence_info(fence).expect("language");
            assert_eq!(lang, expected, "fence: {fence}");
        }
    }

    #[test]
    fn highlights_common_repo_languages_without_error() {
        // This mainly ensures our highlight queries compile + the highlighter doesn't
        // choke on representative snippets.
        let cases: &[(HighlightLanguage, &str)] = &[
            (
                HighlightLanguage::Dart,
                "class A { final int x = 1; A(this.x); }",
            ),
            (
                HighlightLanguage::Sql,
                "select * from drivers where id = 1;",
            ),
            (HighlightLanguage::Java, "class A { int x = 1; }"),
            (HighlightLanguage::Kotlin, "data class A(val x: Int = 1)"),
            (HighlightLanguage::Css, "body { color: red; }"),
            (HighlightLanguage::Html, "<div class='x'>hi</div>"),
            (
                HighlightLanguage::Hcl,
                "resource \"x\" \"y\" { name = \"z\" }",
            ),
            (HighlightLanguage::Markdown, "# Title\n- item\n`code`\n"),
            (HighlightLanguage::Xml, "<a href=\"/\">hi</a>"),
            (HighlightLanguage::Dockerfile, "FROM alpine\nRUN echo hi\n"),
            (HighlightLanguage::Dotenv, "FOO=bar\n# comment\n"),
            (HighlightLanguage::Ini, "[sec]\nkey=value\n"),
        ];

        for (lang, src) in cases {
            let lines = highlight_to_lines(*lang, src);
            assert_eq!(reconstructed(&lines), *src);
        }
    }
}
