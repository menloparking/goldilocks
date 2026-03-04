//! Printer — converts a `Doc` IR tree into a final `String`.
//!
//! Implements a Wadler-Lindig–style algorithm: we walk the document tree
//! maintaining a stack of `(indent, mode, doc)` commands. Groups are first
//! tried in flat mode; if the flat rendering exceeds `max_width` the group
//! switches to break mode and all its softlines become real newlines.

use crate::ir::Doc;
use crate::FormatConfig;

/// Print mode for a document fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Try to fit everything on one line.
    Flat,
    /// Break softlines into real newlines.
    Break,
}

/// A command on the printer's work stack.
#[derive(Debug)]
struct Cmd<'a> {
    indent: usize,
    mode: Mode,
    doc: &'a Doc,
}

/// Render a document tree to a string respecting `config.max_width`.
pub fn print_doc(doc: &Doc, config: &FormatConfig) -> String {
    let mut out = String::new();
    // Position on the current line (number of characters printed since last newline).
    let mut pos: usize = 0;
    // True when the last thing we emitted was a newline + indent (no text yet on this line).
    // Used to strip trailing whitespace from blank lines.
    let mut at_line_start: bool = true;
    // Pending line suffixes (trailing comments etc.) — flushed before each hardline.
    let mut line_suffixes: Vec<Cmd> = Vec::new();
    // Work stack — we process from the back (LIFO).
    let mut stack: Vec<Cmd> = Vec::new();

    stack.push(Cmd {
        indent: 0,
        mode: Mode::Break,
        doc,
    });

    while let Some(cmd) = stack.pop() {
        match cmd.doc {
            Doc::Empty => {}

            Doc::Text(s) => {
                out.push_str(s);
                pos += s.len();
                at_line_start = false;
            }

            Doc::Verbatim(s) => {
                // Verbatim text is printed exactly — but we need to track
                // position for the last line within it.
                out.push_str(s);
                if let Some(last_nl) = s.rfind('\n') {
                    pos = s.len() - last_nl - 1;
                    at_line_start = pos == 0;
                } else {
                    pos += s.len();
                    at_line_start = false;
                }
            }

            Doc::Hardline => {
                // Flush any pending line suffixes first.
                flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                // If we're at the start of a line (no text was printed), trim
                // trailing whitespace from the previous blank line.
                if at_line_start {
                    trim_trailing_whitespace(&mut out);
                }
                out.push('\n');
                out.push_str(&" ".repeat(cmd.indent));
                pos = cmd.indent;
                at_line_start = true;
            }

            Doc::Softline { flat_alt } => {
                match cmd.mode {
                    Mode::Flat => {
                        out.push_str(flat_alt);
                        pos += flat_alt.len();
                        if !flat_alt.is_empty() {
                            at_line_start = false;
                        }
                    }
                    Mode::Break => {
                        // Flush any pending line suffixes first.
                        flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                        if at_line_start {
                            trim_trailing_whitespace(&mut out);
                        }
                        out.push('\n');
                        out.push_str(&" ".repeat(cmd.indent));
                        pos = cmd.indent;
                        at_line_start = true;
                    }
                }
            }

            Doc::Concat(docs) => {
                // Push in reverse so the first child is processed first.
                for d in docs.iter().rev() {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: cmd.mode,
                        doc: d,
                    });
                }
            }

            Doc::Indent(inner) => {
                stack.push(Cmd {
                    indent: cmd.indent + config.indent_width,
                    mode: cmd.mode,
                    doc: inner,
                });
            }

            Doc::Align(n, inner) => {
                stack.push(Cmd {
                    indent: cmd.indent + n,
                    mode: cmd.mode,
                    doc: inner,
                });
            }

            Doc::Group(inner) => {
                // Try flat first. If it fits within max_width, keep flat.
                if cmd.mode == Mode::Flat || fits(inner, cmd.indent, pos, config) {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Flat,
                        doc: inner,
                    });
                } else {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Break,
                        doc: inner,
                    });
                }
            }

            Doc::ConditionalGroup(inner) => {
                // Same as Group for now.
                if cmd.mode == Mode::Flat || fits(inner, cmd.indent, pos, config) {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Flat,
                        doc: inner,
                    });
                } else {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Break,
                        doc: inner,
                    });
                }
            }

            Doc::IfBreak {
                break_contents,
                flat_contents,
            } => {
                match cmd.mode {
                    Mode::Flat => {
                        stack.push(Cmd {
                            indent: cmd.indent,
                            mode: cmd.mode,
                            doc: flat_contents,
                        });
                    }
                    Mode::Break => {
                        stack.push(Cmd {
                            indent: cmd.indent,
                            mode: cmd.mode,
                            doc: break_contents,
                        });
                    }
                }
            }

            Doc::BreakParent => {
                // BreakParent is handled during the `fits` check — if we
                // encounter it while measuring flat, it signals "does not fit".
                // During actual printing it's a no-op.
            }

            Doc::BestOf(variants) => {
                // Try each variant in order. Render to a temporary buffer and
                // check that no line exceeds max_width. Use the first variant
                // that fits. If none fits, fall back to the last variant.
                let chosen = pick_best_variant(variants, cmd.indent, pos, config);
                stack.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: chosen,
                });
            }

            Doc::LineSuffix(inner) => {
                line_suffixes.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: inner,
                });
            }

            Doc::LineSuffixBoundary => {
                if !line_suffixes.is_empty() {
                    flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                    out.push('\n');
                    out.push_str(&" ".repeat(cmd.indent));
                    pos = cmd.indent;
                    at_line_start = true;
                }
            }
        }
    }

    // Flush any remaining line suffixes at end of document.
    flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);

    out
}

/// Flush pending line suffixes into the output.
fn flush_line_suffixes(suffixes: &mut Vec<Cmd>, out: &mut String, pos: &mut usize, at_line_start: &mut bool) {
    // Process in order (they were appended in order).
    let drained: Vec<Cmd> = suffixes.drain(..).collect();
    for cmd in &drained {
        print_flat(cmd.doc, out, pos);
        if *pos > 0 {
            *at_line_start = false;
        }
    }
}

/// Trim trailing whitespace (spaces/tabs) from the output buffer.
/// Used to clean up blank lines that have only indentation.
fn trim_trailing_whitespace(out: &mut String) {
    let trimmed_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
    out.truncate(trimmed_len);
}

/// Print a doc fragment in flat mode (no line breaking) — used for line suffixes.
fn print_flat(doc: &Doc, out: &mut String, pos: &mut usize) {
    match doc {
        Doc::Empty | Doc::BreakParent | Doc::LineSuffixBoundary => {}
        Doc::Text(s) => {
            out.push_str(s);
            *pos += s.len();
        }
        Doc::Verbatim(s) => {
            out.push_str(s);
            if let Some(last_nl) = s.rfind('\n') {
                *pos = s.len() - last_nl - 1;
            } else {
                *pos += s.len();
            }
        }
        Doc::Hardline => {
            out.push('\n');
            *pos = 0;
        }
        Doc::Softline { flat_alt } => {
            out.push_str(flat_alt);
            *pos += flat_alt.len();
        }
        Doc::Concat(docs) => {
            for d in docs {
                print_flat(d, out, pos);
            }
        }
        Doc::Indent(inner) | Doc::Align(_, inner) | Doc::Group(inner) | Doc::ConditionalGroup(inner) => {
            print_flat(inner, out, pos);
        }
        Doc::IfBreak { flat_contents, .. } => {
            print_flat(flat_contents, out, pos);
        }
        Doc::LineSuffix(inner) => {
            print_flat(inner, out, pos);
        }
        Doc::BestOf(variants) => {
            // In flat mode, use the first variant (most compact).
            if let Some(v) = variants.first() {
                print_flat(v, out, pos);
            }
        }
    }
}

// ── Fits check ──────────────────────────────────────────────────────────────

/// Check whether `doc` fits on the remainder of the current line when
/// rendered in flat mode.
fn fits(doc: &Doc, indent: usize, current_pos: usize, config: &FormatConfig) -> bool {
    let remaining = if config.max_width > current_pos {
        config.max_width - current_pos
    } else {
        0
    };
    fits_inner(doc, indent, remaining as isize, config)
}

/// Recursive fits check. Returns `true` if the document fits within
/// `remaining` columns when printed flat.
fn fits_inner(doc: &Doc, indent: usize, remaining: isize, config: &FormatConfig) -> bool {
    if remaining < 0 {
        return false;
    }
    match doc {
        Doc::Empty => true,

        Doc::Text(s) => remaining >= s.len() as isize,

        Doc::Verbatim(s) => {
            // If it contains newlines, check the last line.
            if let Some(last_nl) = s.rfind('\n') {
                let last_line_len = s.len() - last_nl - 1;
                last_line_len <= remaining as usize
            } else {
                remaining >= s.len() as isize
            }
        }

        Doc::Hardline => {
            // A hardline always "fits" — it just means we'll break here.
            true
        }

        Doc::Softline { flat_alt } => remaining >= flat_alt.len() as isize,

        Doc::Concat(docs) => {
            let mut rem = remaining;
            for d in docs {
                if !fits_accum(d, indent, &mut rem, config) {
                    return false;
                }
            }
            true
        }

        Doc::Indent(inner) => fits_inner(inner, indent + config.indent_width, remaining, config),

        Doc::Align(n, inner) => fits_inner(inner, indent + n, remaining, config),

        Doc::Group(inner) | Doc::ConditionalGroup(inner) => {
            fits_inner(inner, indent, remaining, config)
        }

        Doc::IfBreak { flat_contents, .. } => {
            fits_inner(flat_contents, indent, remaining, config)
        }

        Doc::BreakParent => {
            // BreakParent means this group *must* break — so it "doesn't fit" flat.
            false
        }

        Doc::LineSuffix(_) => true, // suffixes don't count toward line width
        Doc::LineSuffixBoundary => true,
        Doc::BestOf(variants) => {
            // In a fits check, try the first variant (most compact).
            variants.first().map_or(true, |v| fits_inner(v, indent, remaining, config))
        }
    }
}

/// Like `fits_inner` but mutates `remaining` to accumulate width consumption
/// across a sequence of docs.
fn fits_accum(doc: &Doc, indent: usize, remaining: &mut isize, config: &FormatConfig) -> bool {
    if *remaining < 0 {
        return false;
    }
    match doc {
        Doc::Empty => true,

        Doc::Text(s) => {
            *remaining -= s.len() as isize;
            *remaining >= 0
        }

        Doc::Verbatim(s) => {
            if let Some(last_nl) = s.rfind('\n') {
                let last_line_len = s.len() - last_nl - 1;
                *remaining = config.max_width as isize - indent as isize - last_line_len as isize;
            } else {
                *remaining -= s.len() as isize;
            }
            *remaining >= 0
        }

        Doc::Hardline => {
            // After a hardline, remaining resets.
            *remaining = config.max_width as isize - indent as isize;
            true
        }

        Doc::Softline { flat_alt } => {
            *remaining -= flat_alt.len() as isize;
            *remaining >= 0
        }

        Doc::Concat(docs) => {
            for d in docs {
                if !fits_accum(d, indent, remaining, config) {
                    return false;
                }
            }
            true
        }

        Doc::Indent(inner) => fits_accum(inner, indent + config.indent_width, remaining, config),
        Doc::Align(n, inner) => fits_accum(inner, indent + n, remaining, config),

        Doc::Group(inner) | Doc::ConditionalGroup(inner) => {
            fits_accum(inner, indent, remaining, config)
        }

        Doc::IfBreak { flat_contents, .. } => {
            fits_accum(flat_contents, indent, remaining, config)
        }

        Doc::BreakParent => false,

        Doc::LineSuffix(_) | Doc::LineSuffixBoundary => true,
        Doc::BestOf(variants) => {
            // In fits_accum, try the first variant (most compact).
            variants.first().map_or(true, |v| fits_accum(v, indent, remaining, config))
        }
    }
}

// ── BestOf variant selection ────────────────────────────────────────────────

/// Render a Doc variant to a string, starting at the given indent and position.
/// Returns the rendered text fragment (not including any prior output).
fn render_variant(doc: &Doc, indent: usize, start_pos: usize, config: &FormatConfig) -> String {
    // We reuse print_doc's logic but with an initial indent/position context.
    // To achieve this, we run the same stack-based algorithm.
    let mut out = String::new();
    let mut pos = start_pos;
    let mut at_line_start = start_pos == 0;
    let mut line_suffixes: Vec<Cmd> = Vec::new();
    let mut stack: Vec<Cmd> = vec![Cmd {
        indent,
        mode: Mode::Break,
        doc,
    }];

    while let Some(cmd) = stack.pop() {
        match cmd.doc {
            Doc::Empty => {}
            Doc::Text(s) => {
                out.push_str(s);
                pos += s.len();
                at_line_start = false;
            }
            Doc::Verbatim(s) => {
                out.push_str(s);
                if let Some(last_nl) = s.rfind('\n') {
                    pos = s.len() - last_nl - 1;
                    at_line_start = pos == 0;
                } else {
                    pos += s.len();
                    at_line_start = false;
                }
            }
            Doc::Hardline => {
                flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                if at_line_start {
                    trim_trailing_whitespace(&mut out);
                }
                out.push('\n');
                out.push_str(&" ".repeat(cmd.indent));
                pos = cmd.indent;
                at_line_start = true;
            }
            Doc::Softline { flat_alt } => match cmd.mode {
                Mode::Flat => {
                    out.push_str(flat_alt);
                    pos += flat_alt.len();
                    if !flat_alt.is_empty() {
                        at_line_start = false;
                    }
                }
                Mode::Break => {
                    flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                    if at_line_start {
                        trim_trailing_whitespace(&mut out);
                    }
                    out.push('\n');
                    out.push_str(&" ".repeat(cmd.indent));
                    pos = cmd.indent;
                    at_line_start = true;
                }
            },
            Doc::Concat(docs) => {
                for d in docs.iter().rev() {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: cmd.mode,
                        doc: d,
                    });
                }
            }
            Doc::Indent(inner) => {
                stack.push(Cmd {
                    indent: cmd.indent + config.indent_width,
                    mode: cmd.mode,
                    doc: inner,
                });
            }
            Doc::Align(n, inner) => {
                stack.push(Cmd {
                    indent: cmd.indent + n,
                    mode: cmd.mode,
                    doc: inner,
                });
            }
            Doc::Group(inner) => {
                if cmd.mode == Mode::Flat || fits(inner, cmd.indent, pos, config) {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Flat,
                        doc: inner,
                    });
                } else {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Break,
                        doc: inner,
                    });
                }
            }
            Doc::ConditionalGroup(inner) => {
                if cmd.mode == Mode::Flat || fits(inner, cmd.indent, pos, config) {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Flat,
                        doc: inner,
                    });
                } else {
                    stack.push(Cmd {
                        indent: cmd.indent,
                        mode: Mode::Break,
                        doc: inner,
                    });
                }
            }
            Doc::IfBreak {
                break_contents,
                flat_contents,
            } => match cmd.mode {
                Mode::Flat => stack.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: flat_contents,
                }),
                Mode::Break => stack.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: break_contents,
                }),
            },
            Doc::BreakParent => {}
            Doc::BestOf(variants) => {
                let chosen = pick_best_variant(variants, cmd.indent, pos, config);
                stack.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: chosen,
                });
            }
            Doc::LineSuffix(inner) => {
                line_suffixes.push(Cmd {
                    indent: cmd.indent,
                    mode: cmd.mode,
                    doc: inner,
                });
            }
            Doc::LineSuffixBoundary => {
                if !line_suffixes.is_empty() {
                    flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
                    out.push('\n');
                    out.push_str(&" ".repeat(cmd.indent));
                    pos = cmd.indent;
                    at_line_start = true;
                }
            }
        }
    }

    flush_line_suffixes(&mut line_suffixes, &mut out, &mut pos, &mut at_line_start);
    out
}

/// Check whether every line in `text` (starting at column `start_pos` for the
/// first line) fits within `max_width`.
fn all_lines_fit(text: &str, start_pos: usize, max_width: usize) -> bool {
    for (i, line) in text.split('\n').enumerate() {
        let width = if i == 0 {
            start_pos + line.len()
        } else {
            line.len()
        };
        if width > max_width {
            return false;
        }
    }
    true
}

/// Pick the best variant from a BestOf list. Renders each variant (except the
/// last) and returns a reference to the first one that fits within max_width.
/// If none fits, returns the last variant.
fn pick_best_variant<'a>(
    variants: &'a [Doc],
    indent: usize,
    pos: usize,
    config: &FormatConfig,
) -> &'a Doc {
    let last_idx = variants.len() - 1;
    for (i, variant) in variants.iter().enumerate() {
        if i == last_idx {
            // Last variant is the fallback — always use it.
            return variant;
        }
        let rendered = render_variant(variant, indent, pos, config);
        if all_lines_fit(&rendered, pos, config.max_width) {
            return variant;
        }
    }
    // Should never reach here, but just in case.
    variants.last().unwrap()
}
