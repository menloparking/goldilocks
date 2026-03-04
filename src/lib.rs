//! Goldilocks — a Ruby beautifier built on Prism.
//!
//! Parses Ruby source using the official Prism parser (ruby-prism crate),
//! converts the AST into a pretty-printing intermediate representation,
//! then prints it respecting a configurable column width (default 100).

pub mod ir;
pub mod formatter;
pub mod printer;

/// Configuration for the formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Maximum line width before rewrapping kicks in.
    pub max_width: usize,
    /// Number of spaces per indentation level.
    pub indent_width: usize,
    /// Whether to add trailing newline.
    pub trailing_newline: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            max_width: 100,
            indent_width: 2,
            trailing_newline: true,
        }
    }
}

/// Format a Ruby source string, returning the beautified output.
///
/// Returns `Err` if there are parse errors that prevent formatting.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let parse_result = ruby_prism::parse(source.as_bytes());

    let errors: Vec<_> = parse_result.errors().collect();
    if !errors.is_empty() {
        let messages: Vec<String> = errors
            .iter()
            .map(|e| format!("{:?}", e))
            .collect();
        return Err(FormatError::ParseError(messages.join("\n")));
    }

    let node = parse_result.node();
    let source_bytes = source.as_bytes();

    // Collect comments from the parse result.
    let comments: Vec<formatter::CommentInfo> = parse_result
        .comments()
        .map(|c| {
            let loc = c.location();
            let text = String::from_utf8_lossy(c.text()).into_owned();
            let is_inline = c.type_() == ruby_prism::CommentType::InlineComment;
            formatter::CommentInfo {
                start_offset: loc.start_offset(),
                end_offset: loc.end_offset(),
                text,
                is_inline,
            }
        })
        .collect();

    let doc = formatter::format_program(&node, source_bytes, &comments, config);
    let mut output = printer::print_doc(&doc, config);

    // Ensure trailing newline
    if config.trailing_newline && !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

/// Errors that can occur during formatting.
#[derive(Debug)]
pub enum FormatError {
    ParseError(String),
    IoError(std::io::Error),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            FormatError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::IoError(e)
    }
}
