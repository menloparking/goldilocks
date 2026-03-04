//! Intermediate representation for pretty-printing.
//!
//! This is a Wadler-Lindig style document algebra. The formatter builds a `Doc`
//! tree, and the printer consumes it to produce the final string output,
//! choosing where to break lines based on available width.

/// A pretty-printing document.
///
/// Documents are built by the formatter and consumed by the printer.
/// They form an immutable tree describing *what* to print and *where*
/// line breaks are permitted.
#[derive(Debug, Clone)]
pub enum Doc {
    /// Literal text — printed exactly as-is, never broken.
    Text(String),

    /// A hard line break — always produces a newline + current indentation.
    Hardline,

    /// A "soft" line break:
    /// - In **flat** mode: prints `flat_alt` (typically `" "` or `""`).
    /// - In **break** mode: prints a newline + current indentation.
    Softline { flat_alt: String },

    /// A sequence of documents printed one after another.
    Concat(Vec<Doc>),

    /// Increase the indentation level for the contents.
    Indent(Box<Doc>),

    /// Increase indentation by an arbitrary number of spaces (for alignment).
    Align(usize, Box<Doc>),

    /// A group: the printer first tries to print the entire group flat
    /// (on one line). If it doesn't fit, it breaks — all `Softline`s
    /// inside become newlines.
    Group(Box<Doc>),

    /// Like `Group`, but when this group breaks, *only* the immediately
    /// contained softlines break — nested groups still try flat first.
    /// This is rarely needed; normal `Group` suffices for most cases.
    ConditionalGroup(Box<Doc>),

    /// Content that should only appear when the enclosing group breaks.
    BreakParent,

    /// An if-break construct:
    /// - `break_contents`: printed when the enclosing group is broken.
    /// - `flat_contents`: printed when the enclosing group is flat.
    IfBreak {
        break_contents: Box<Doc>,
        flat_contents: Box<Doc>,
    },

    /// A line suffix — content that is deferred to the end of the current
    /// line (used for trailing comments).
    LineSuffix(Box<Doc>),

    /// Force a line break after any pending line suffixes.
    LineSuffixBoundary,

    /// Verbatim source text — printed exactly as-is, preserving internal
    /// newlines. Used for heredocs and other untouchable content.
    Verbatim(String),

    /// Try multiple layout variants in order — the printer picks the first
    /// one where no line exceeds `max_width`. If none fits, the last variant
    /// (typically the most broken) is used.
    ///
    /// This enables "solution exploration" at strategic points: the formatter
    /// generates 2–3 candidate layouts and lets the printer resolve which one
    /// actually fits within the column budget at print time.
    BestOf(Vec<Doc>),

    /// An empty document — prints nothing.
    Empty,
}

// ── Convenience constructors ────────────────────────────────────────────────

impl Doc {
    /// Literal text.
    pub fn text(s: impl Into<String>) -> Doc {
        let s = s.into();
        if s.is_empty() {
            Doc::Empty
        } else {
            Doc::Text(s)
        }
    }

    /// A hard line break (always breaks).
    pub fn hardline() -> Doc {
        Doc::Hardline
    }

    /// A soft line: prints `" "` when flat, newline when broken.
    pub fn softline() -> Doc {
        Doc::Softline {
            flat_alt: " ".into(),
        }
    }

    /// A soft line that prints `""` when flat (i.e., nothing), newline when broken.
    pub fn softline_empty() -> Doc {
        Doc::Softline {
            flat_alt: String::new(),
        }
    }

    /// A line that prints the given string when flat.
    pub fn line_or(flat: impl Into<String>) -> Doc {
        Doc::Softline {
            flat_alt: flat.into(),
        }
    }

    /// Concatenate multiple docs.
    pub fn concat(docs: Vec<Doc>) -> Doc {
        // Flatten nested concats and strip empties.
        let mut flat = Vec::with_capacity(docs.len());
        for d in docs {
            match d {
                Doc::Empty => {}
                Doc::Concat(inner) => {
                    for d2 in inner {
                        if !matches!(d2, Doc::Empty) {
                            flat.push(d2);
                        }
                    }
                }
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Doc::Empty,
            1 => flat.into_iter().next().unwrap(),
            _ => Doc::Concat(flat),
        }
    }

    /// Indent contents by one level.
    pub fn indent(doc: Doc) -> Doc {
        Doc::Indent(Box::new(doc))
    }

    /// Indent contents by a specific number of spaces (alignment).
    pub fn align(n: usize, doc: Doc) -> Doc {
        if n == 0 {
            doc
        } else {
            Doc::Align(n, Box::new(doc))
        }
    }

    /// Group: try flat, break if it doesn't fit.
    pub fn group(doc: Doc) -> Doc {
        Doc::Group(Box::new(doc))
    }

    /// If-break: choose between two docs based on whether the enclosing
    /// group breaks.
    pub fn if_break(break_doc: Doc, flat_doc: Doc) -> Doc {
        Doc::IfBreak {
            break_contents: Box::new(break_doc),
            flat_contents: Box::new(flat_doc),
        }
    }

    /// A trailing comment or other line suffix.
    pub fn line_suffix(doc: Doc) -> Doc {
        Doc::LineSuffix(Box::new(doc))
    }

    /// Verbatim text — preserved exactly, including internal newlines.
    pub fn verbatim(s: impl Into<String>) -> Doc {
        Doc::Verbatim(s.into())
    }

    /// Force the enclosing group to break.
    pub fn break_parent() -> Doc {
        Doc::BreakParent
    }

    /// Try multiple layout variants — the printer picks the first one that
    /// fits within `max_width`. Variants should be ordered from most-compact
    /// (flattest) to most-broken. If none fits, the last variant is used.
    pub fn best_of(variants: Vec<Doc>) -> Doc {
        assert!(!variants.is_empty(), "BestOf requires at least one variant");
        if variants.len() == 1 {
            variants.into_iter().next().unwrap()
        } else {
            Doc::BestOf(variants)
        }
    }
}

// ── Join helper ─────────────────────────────────────────────────────────────

/// Join an iterator of docs with a separator doc between each pair.
pub fn join(sep: Doc, docs: impl IntoIterator<Item = Doc>) -> Doc {
    let mut result: Vec<Doc> = Vec::new();
    let mut first = true;
    for doc in docs {
        if !first {
            result.push(sep.clone());
        }
        first = false;
        result.push(doc);
    }
    Doc::concat(result)
}

/// Join docs with a comma + softline separator, suitable for argument lists.
pub fn join_comma_separated(docs: impl IntoIterator<Item = Doc>) -> Doc {
    let mut result: Vec<Doc> = Vec::new();
    let mut first = true;
    for doc in docs {
        if !first {
            result.push(Doc::text(","));
            result.push(Doc::softline());
        }
        first = false;
        result.push(doc);
    }
    Doc::concat(result)
}
