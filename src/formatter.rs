//! Formatter — converts Prism AST nodes into a `Doc` IR tree.
//!
//! This is the core of goldilocks: each Ruby AST node is converted into a
//! pretty-printing document that the printer can then render with
//! width-aware line breaking.

use std::cell::RefCell;
use ruby_prism::Node;

use crate::ir::{join_comma_separated, Doc};
use crate::FormatConfig;

/// Information about a source comment.
#[derive(Debug, Clone)]
pub struct CommentInfo {
    pub start_offset: usize,
    pub end_offset: usize,
    /// The full comment text including the `#` prefix.
    pub text: String,
    /// `true` for `# ...` inline comments, `false` for `=begin...=end` embedded docs.
    pub is_inline: bool,
}

// Thread-local comment tracker so we don't need to thread it through every function.
thread_local! {
    static COMMENT_TRACKER: RefCell<Option<CommentTrackerInner>> = RefCell::new(None);
}

struct CommentTrackerInner {
    comments: Vec<CommentInfo>,
    consumed: Vec<bool>,
}

/// Install comments for the current formatting pass, run the closure, then clean up.
fn with_comments<F: FnOnce() -> Doc>(comments: &[CommentInfo], f: F) -> Doc {
    COMMENT_TRACKER.with(|cell| {
        *cell.borrow_mut() = Some(CommentTrackerInner {
            comments: comments.to_vec(),
            consumed: vec![false; comments.len()],
        });
    });
    let result = f();
    COMMENT_TRACKER.with(|cell| {
        *cell.borrow_mut() = None;
    });
    result
}

/// Return all comments whose start_offset is in `[from, to)` that haven't
/// been consumed yet, and mark them consumed.
fn take_comments_between(from: usize, to: usize) -> Vec<CommentInfo> {
    COMMENT_TRACKER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(ref mut tracker) = *borrow else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for (i, c) in tracker.comments.iter().enumerate() {
            if !tracker.consumed[i] && c.start_offset >= from && c.start_offset < to {
                tracker.consumed[i] = true;
                result.push(c.clone());
            }
        }
        result
    })
}

/// Return all remaining unconsumed comments.
fn take_remaining_comments() -> Vec<CommentInfo> {
    COMMENT_TRACKER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(ref mut tracker) = *borrow else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for (i, c) in tracker.comments.iter().enumerate() {
            if !tracker.consumed[i] {
                tracker.consumed[i] = true;
                result.push(c.clone());
            }
        }
        result
    })
}

/// Format a program node — the top-level entry point that accepts comments.
pub fn format_program(
    node: &Node<'_>,
    source: &[u8],
    comments: &[CommentInfo],
    config: &FormatConfig,
) -> Doc {
    let doc = with_comments(comments, || {
        let main = format_node(node, source, config);
        // Emit any remaining comments at the end (while tracker is still active)
        let remaining = take_remaining_comments();
        if remaining.is_empty() {
            main
        } else {
            let mut parts = vec![main];
            for c in remaining {
                parts.push(Doc::hardline());
                parts.push(Doc::text(c.text.trim_end()));
            }
            Doc::concat(parts)
        }
    });
    doc
}

/// Format an AST node into a `Doc`.
fn format_node(node: &Node<'_>, source: &[u8], config: &FormatConfig) -> Doc {
    match node {
        Node::ProgramNode { .. } => {
            let n = node.as_program_node().unwrap();
            let stmts = n.statements();
            format_statements_body(&stmts.body(), source, config)
        }

        Node::StatementsNode { .. } => {
            let n = node.as_statements_node().unwrap();
            format_statements_body(&n.body(), source, config)
        }

        // ── Literals ────────────────────────────────────────────────────

        Node::IntegerNode { .. }
        | Node::FloatNode { .. }
        | Node::RationalNode { .. }
        | Node::ImaginaryNode { .. }
        | Node::NilNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::SelfNode { .. }
        | Node::SourceFileNode { .. }
        | Node::SourceLineNode { .. }
        | Node::SourceEncodingNode { .. }
        | Node::RedoNode { .. }
        | Node::RetryNode { .. }
        | Node::ForwardingArgumentsNode { .. }
        | Node::ForwardingParameterNode { .. }
        | Node::ItLocalVariableReadNode { .. }
        | Node::MissingNode { .. }
        | Node::ImplicitRestNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Variables (reads) ───────────────────────────────────────────

        Node::LocalVariableReadNode { .. }
        | Node::InstanceVariableReadNode { .. }
        | Node::ClassVariableReadNode { .. }
        | Node::GlobalVariableReadNode { .. }
        | Node::ConstantReadNode { .. }
        | Node::BackReferenceReadNode { .. }
        | Node::NumberedReferenceReadNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Variable writes ─────────────────────────────────────────────

        Node::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node().unwrap();
            format_simple_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }

        Node::InstanceVariableWriteNode { .. } => {
            let n = node.as_instance_variable_write_node().unwrap();
            format_simple_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }

        Node::ClassVariableWriteNode { .. } => {
            let n = node.as_class_variable_write_node().unwrap();
            format_simple_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }

        Node::GlobalVariableWriteNode { .. } => {
            let n = node.as_global_variable_write_node().unwrap();
            format_simple_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }

        Node::ConstantWriteNode { .. } => {
            let n = node.as_constant_write_node().unwrap();
            format_simple_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }

        Node::ConstantPathWriteNode { .. } => {
            let n = node.as_constant_path_write_node().unwrap();
            let target = format_node(&n.target().as_node(), source, config);
            let op = loc_str(n.operator_loc(), source);
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![target, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
        }

        // ── Compound writes (&&=, ||=, op=) ────────────────────────────

        Node::LocalVariableAndWriteNode { .. } => {
            let n = node.as_local_variable_and_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let n = node.as_local_variable_or_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let n = node.as_local_variable_operator_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.binary_operator_loc(), source), &n.value(), source, config)
        }

        Node::InstanceVariableAndWriteNode { .. } => {
            let n = node.as_instance_variable_and_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::InstanceVariableOrWriteNode { .. } => {
            let n = node.as_instance_variable_or_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::InstanceVariableOperatorWriteNode { .. } => {
            let n = node.as_instance_variable_operator_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.binary_operator_loc(), source), &n.value(), source, config)
        }

        Node::ClassVariableAndWriteNode { .. } => {
            let n = node.as_class_variable_and_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::ClassVariableOrWriteNode { .. } => {
            let n = node.as_class_variable_or_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::ClassVariableOperatorWriteNode { .. } => {
            let n = node.as_class_variable_operator_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.binary_operator_loc(), source), &n.value(), source, config)
        }

        Node::GlobalVariableAndWriteNode { .. } => {
            let n = node.as_global_variable_and_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::GlobalVariableOrWriteNode { .. } => {
            let n = node.as_global_variable_or_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::GlobalVariableOperatorWriteNode { .. } => {
            let n = node.as_global_variable_operator_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.binary_operator_loc(), source), &n.value(), source, config)
        }

        Node::ConstantAndWriteNode { .. } => {
            let n = node.as_constant_and_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::ConstantOrWriteNode { .. } => {
            let n = node.as_constant_or_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.operator_loc(), source), &n.value(), source, config)
        }
        Node::ConstantOperatorWriteNode { .. } => {
            let n = node.as_constant_operator_write_node().unwrap();
            format_compound_write(loc_str(n.name_loc(), source), loc_str(n.binary_operator_loc(), source), &n.value(), source, config)
        }

        Node::ConstantPathAndWriteNode { .. } => {
            let n = node.as_constant_path_and_write_node().unwrap();
            let target = format_node(&n.target().as_node(), source, config);
            let op = loc_str(n.operator_loc(), source);
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![target, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
        }
        Node::ConstantPathOrWriteNode { .. } => {
            let n = node.as_constant_path_or_write_node().unwrap();
            let target = format_node(&n.target().as_node(), source, config);
            let op = loc_str(n.operator_loc(), source);
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![target, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
        }
        Node::ConstantPathOperatorWriteNode { .. } => {
            let n = node.as_constant_path_operator_write_node().unwrap();
            let target = format_node(&n.target().as_node(), source, config);
            let op = loc_str(n.binary_operator_loc(), source);
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![target, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
        }

        // Variable targets (for multi-assignment LHS)
        Node::LocalVariableTargetNode { .. }
        | Node::InstanceVariableTargetNode { .. }
        | Node::ClassVariableTargetNode { .. }
        | Node::GlobalVariableTargetNode { .. }
        | Node::ConstantTargetNode { .. }
        | Node::ConstantPathTargetNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Constant path (A::B::C) ────────────────────────────────────

        Node::ConstantPathNode { .. } => {
            let n = node.as_constant_path_node().unwrap();
            let mut parts = Vec::new();
            if let Some(parent) = n.parent() {
                parts.push(format_node(&parent, source, config));
            }
            parts.push(Doc::text(loc_str(n.delimiter_loc(), source)));
            if let Some(name) = n.name() {
                parts.push(Doc::text(name_str(&name)));
            }
            Doc::concat(parts)
        }

        // ── Strings ────────────────────────────────────────────────────

        Node::StringNode { .. } => {
            let n = node.as_string_node().unwrap();
            if is_heredoc_opening(n.opening_loc(), source) {
                return format_heredoc_string_node(n, source);
            }
            normalize_string_quotes(node.location(), n.opening_loc(), n.content_loc(), source)
        }

        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            if is_heredoc_opening(n.opening_loc(), source) {
                return format_heredoc_interpolated_string_node(n, source);
            }
            // Format parts individually so string literals inside #{} get
            // quote-normalized (StandardRB Style/StringLiteralsInInterpolation).
            let mut parts = Vec::new();
            if let Some(open_loc) = n.opening_loc() {
                parts.push(Doc::text(loc_str(open_loc, source)));
            }
            for part in n.parts().iter() {
                parts.push(format_node(&part, source, config));
            }
            if let Some(close_loc) = n.closing_loc() {
                parts.push(Doc::text(loc_str(close_loc, source)));
            }
            Doc::concat(parts)
        }

        Node::XStringNode { .. } => {
            let n = node.as_x_string_node().unwrap();
            if is_heredoc_opening(Some(n.opening_loc()), source) {
                return format_heredoc_xstring_node(n, source);
            }
            Doc::text(loc_str(node.location(), source))
        }

        Node::InterpolatedXStringNode { .. } => {
            let n = node.as_interpolated_x_string_node().unwrap();
            if is_heredoc_opening(Some(n.opening_loc()), source) {
                return format_heredoc_interpolated_xstring_node(n, source);
            }
            Doc::text(loc_str(node.location(), source))
        }

        // ── Symbols ────────────────────────────────────────────────────

        Node::SymbolNode { .. }
        | Node::InterpolatedSymbolNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Regular expressions ─────────────────────────────────────────

        Node::RegularExpressionNode { .. }
        | Node::InterpolatedRegularExpressionNode { .. }
        | Node::MatchLastLineNode { .. }
        | Node::InterpolatedMatchLastLineNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Embedded in strings/regex ───────────────────────────────────

        Node::EmbeddedStatementsNode { .. } => {
            let n = node.as_embedded_statements_node().unwrap();
            let opening = loc_str(n.opening_loc(), source);
            let closing = loc_str(n.closing_loc(), source);
            let body = if let Some(stmts) = n.statements() {
                format_node(&stmts.as_node(), source, config)
            } else {
                Doc::Empty
            };
            Doc::concat(vec![Doc::text(opening), body, Doc::text(closing)])
        }

        Node::EmbeddedVariableNode { .. } => {
            let n = node.as_embedded_variable_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            let var = format_node(&n.variable(), source, config);
            Doc::concat(vec![Doc::text(op), var])
        }

        // ── Arrays ─────────────────────────────────────────────────────

        Node::ArrayNode { .. } => {
            let n = node.as_array_node().unwrap();
            let opening = n.opening_loc().map(|l| loc_str(l, source));
            let closing = n.closing_loc().map(|l| loc_str(l, source));

            // Check for %w/%i style arrays — reconstruct with normalized spacing.
            if let Some(ref open) = opening {
                if open.starts_with("%w") || open.starts_with("%W")
                    || open.starts_with("%i") || open.starts_with("%I")
                {
                    // Extract the delimiter: %w[, %i(, %w{, %w<, or %w followed by a custom char.
                    let close_str = closing.as_deref().unwrap_or("]");
                    // Collect element text, normalizing whitespace.
                    let element_texts: Vec<String> = n
                        .elements()
                        .iter()
                        .map(|e| {
                            // Each element in a %w/%i array is a SymbolNode or StringNode.
                            // Extract the unescaped content.
                            let raw = loc_str(e.location(), source);
                            // Normalize: collapse any internal whitespace/newlines.
                            raw.split_whitespace().collect::<Vec<_>>().join(" ")
                        })
                        .collect();
                    return Doc::text(format!("{}{}{}", open, element_texts.join(" "), close_str));
                }
            }

            let elements: Vec<Doc> = n
                .elements()
                .iter()
                .map(|e| format_node(&e, source, config))
                .collect();

            if elements.is_empty() {
                return Doc::text(opening.unwrap_or_else(|| "[]".into()));
            }

            let open = opening.unwrap_or_else(|| "[".into());
            let close = closing.unwrap_or_else(|| "]".into());

            format_bracket_body(&open, &close, elements)
        }

        // ── Hashes ─────────────────────────────────────────────────────

        Node::HashNode { .. } => {
            let n = node.as_hash_node().unwrap();
            let opening = loc_str(n.opening_loc(), source);
            let closing = loc_str(n.closing_loc(), source);

            let elements: Vec<Doc> = n
                .elements()
                .iter()
                .map(|e| format_node(&e, source, config))
                .collect();

            if elements.is_empty() {
                return Doc::concat(vec![Doc::text(opening), Doc::text(closing)]);
            }

            format_hash_body(&opening, &closing, elements)
        }

        Node::KeywordHashNode { .. } => {
            let n = node.as_keyword_hash_node().unwrap();
            let elements: Vec<Doc> = n
                .elements()
                .iter()
                .map(|e| format_node(&e, source, config))
                .collect();
            join_comma_separated(elements)
        }

        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            let key = format_node(&n.key(), source, config);
            let value = format_node(&n.value(), source, config);
            if let Some(op_loc) = n.operator_loc() {
                let op = loc_str(op_loc, source);
                Doc::concat(vec![key, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
            } else {
                // Symbol key shorthand: `foo: bar`
                Doc::concat(vec![key, Doc::text(" "), value])
            }
        }

        Node::AssocSplatNode { .. } => {
            let n = node.as_assoc_splat_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(val) = n.value() {
                Doc::concat(vec![Doc::text(op), format_node(&val, source, config)])
            } else {
                Doc::text(op)
            }
        }

        // ── Splat ──────────────────────────────────────────────────────

        Node::SplatNode { .. } => {
            let n = node.as_splat_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(expr) = n.expression() {
                Doc::concat(vec![Doc::text(op), format_node(&expr, source, config)])
            } else {
                Doc::text(op)
            }
        }

        // ── Range ──────────────────────────────────────────────────────

        Node::RangeNode { .. } => {
            let n = node.as_range_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            let mut parts = Vec::new();
            if let Some(left) = n.left() {
                parts.push(format_node(&left, source, config));
            }
            parts.push(Doc::text(op));
            if let Some(right) = n.right() {
                parts.push(format_node(&right, source, config));
            }
            Doc::concat(parts)
        }

        // ── Method definitions ──────────────────────────────────────────

        Node::DefNode { .. } => {
            let n = node.as_def_node().unwrap();
            let mut parts = Vec::new();

            parts.push(Doc::text("def"));

            // Receiver (e.g., self.method)
            if let Some(recv) = n.receiver() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&recv, source, config));
                if let Some(op_loc) = n.operator_loc() {
                    parts.push(Doc::text(loc_str(op_loc, source)));
                }
            } else {
                parts.push(Doc::text(" "));
            }

            // Name
            parts.push(Doc::text(name_str(&n.name())));

            // Parameters
            if let Some(params) = n.parameters() {
                let has_parens = n.lparen_loc().is_some();
                let param_docs = format_parameters(&params, source, config);

                if has_parens {
                    parts.push(format_paren_args(param_docs));
                } else {
                    parts.push(Doc::text(" "));
                    parts.push(join_comma_separated(param_docs));
                }
            } else if n.lparen_loc().is_some() {
                parts.push(Doc::text("()"));
            }

            // Endless method: def foo = expr
            if let Some(eq_loc) = n.equal_loc() {
                parts.push(Doc::text(" "));
                parts.push(Doc::text(loc_str(eq_loc, source)));
                if let Some(body) = n.body() {
                    parts.push(Doc::text(" "));
                    parts.push(format_node(&body, source, config));
                }
                return Doc::concat(parts);
            }

            // Body
            if let Some(body) = n.body() {
                parts.push(indent_body(format_body(&body, source, config)));
            } else if n.end_keyword_loc().is_some() {
                // No body — but there might be comments inside.
                let body_start = n.lparen_loc()
                    .and_then(|_| n.rparen_loc())
                    .map(|l| l.end_offset())
                    .or_else(|| n.parameters().map(|p| p.location().end_offset()))
                    .unwrap_or(n.name_loc().end_offset());
                let body_end = n.end_keyword_loc().unwrap().start_offset();
                let interior = format_interior_comments(body_start, body_end);
                if !matches!(interior, Doc::Empty) {
                    parts.push(indent_body(interior));
                }
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));

            Doc::concat(parts)
        }

        // ── Class ──────────────────────────────────────────────────────

        Node::ClassNode { .. } => {
            let n = node.as_class_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("class "));
            parts.push(format_node(&n.constant_path(), source, config));

            if let Some(superclass) = n.superclass() {
                parts.push(Doc::text(" < "));
                parts.push(format_node(&superclass, source, config));
            }

            if let Some(body) = n.body() {
                parts.push(indent_body(format_body(&body, source, config)));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));

            Doc::concat(parts)
        }

        // ── Module ─────────────────────────────────────────────────────

        Node::ModuleNode { .. } => {
            let n = node.as_module_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("module "));
            parts.push(format_node(&n.constant_path(), source, config));

            if let Some(body) = n.body() {
                parts.push(indent_body(format_body(&body, source, config)));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));

            Doc::concat(parts)
        }

        // ── Singleton class (class << self) ─────────────────────────────

        Node::SingletonClassNode { .. } => {
            let n = node.as_singleton_class_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("class "));
            parts.push(Doc::text(loc_str(n.operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.expression(), source, config));

            if let Some(body) = n.body() {
                parts.push(indent_body(format_body(&body, source, config)));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));

            Doc::concat(parts)
        }

        // ── Call node ──────────────────────────────────────────────────

        Node::CallNode { .. } => {
            let n = node.as_call_node().unwrap();
            format_call_node(&n, source, config)
        }

        // ── Call-based writes (a.b &&= c, etc.) ────────────────────────

        Node::CallAndWriteNode { .. } => {
            let n = node.as_call_and_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
                if let Some(op) = n.call_operator_loc() {
                    parts.push(Doc::text(loc_str(op, source)));
                }
            }
            if let Some(msg) = n.message_loc() {
                parts.push(Doc::text(loc_str(msg, source)));
            }
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::CallOrWriteNode { .. } => {
            let n = node.as_call_or_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
                if let Some(op) = n.call_operator_loc() {
                    parts.push(Doc::text(loc_str(op, source)));
                }
            }
            if let Some(msg) = n.message_loc() {
                parts.push(Doc::text(loc_str(msg, source)));
            }
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::CallOperatorWriteNode { .. } => {
            let n = node.as_call_operator_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
                if let Some(op) = n.call_operator_loc() {
                    parts.push(Doc::text(loc_str(op, source)));
                }
            }
            if let Some(msg) = n.message_loc() {
                parts.push(Doc::text(loc_str(msg, source)));
            }
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.binary_operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::CallTargetNode { .. } => {
            let n = node.as_call_target_node().unwrap();
            let mut parts = Vec::new();
            parts.push(format_node(&n.receiver(), source, config));
            parts.push(Doc::text(loc_str(n.call_operator_loc(), source)));
            parts.push(Doc::text(name_str(&n.name())));
            Doc::concat(parts)
        }

        // ── Index (a[b], a[b] = c, etc.) ───────────────────────────────

        Node::IndexAndWriteNode { .. } => {
            let n = node.as_index_and_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
            }
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(args) = n.arguments() {
                let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                parts.push(join_comma_separated(arg_docs));
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::IndexOrWriteNode { .. } => {
            let n = node.as_index_or_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
            }
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(args) = n.arguments() {
                let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                parts.push(join_comma_separated(arg_docs));
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::IndexOperatorWriteNode { .. } => {
            let n = node.as_index_operator_write_node().unwrap();
            let mut parts = Vec::new();
            if let Some(recv) = n.receiver() {
                parts.push(format_node(&recv, source, config));
            }
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(args) = n.arguments() {
                let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                parts.push(join_comma_separated(arg_docs));
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(Doc::text(loc_str(n.binary_operator_loc(), source)));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.value(), source, config));
            Doc::concat(parts)
        }

        Node::IndexTargetNode { .. } => {
            let n = node.as_index_target_node().unwrap();
            let mut parts = Vec::new();
            parts.push(format_node(&n.receiver(), source, config));
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(args) = n.arguments() {
                let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                parts.push(join_comma_separated(arg_docs));
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            Doc::concat(parts)
        }

        // ── Arguments ──────────────────────────────────────────────────

        Node::ArgumentsNode { .. } => {
            let n = node.as_arguments_node().unwrap();
            let args: Vec<Doc> = n
                .arguments()
                .iter()
                .map(|a| format_node(&a, source, config))
                .collect();
            join_comma_separated(args)
        }

        Node::BlockArgumentNode { .. } => {
            let n = node.as_block_argument_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(expr) = n.expression() {
                Doc::concat(vec![Doc::text(op), format_node(&expr, source, config)])
            } else {
                Doc::text(op)
            }
        }

        // ── Blocks (do..end / { }) ──────────────────────────────────────

        Node::BlockNode { .. } => {
            let n = node.as_block_node().unwrap();
            format_block_node(&n, source, config)
        }

        Node::LambdaNode { .. } => {
            let n = node.as_lambda_node().unwrap();
            let mut parts = Vec::new();
            let opening = loc_str(n.opening_loc(), source);
            let closing = loc_str(n.closing_loc(), source);
            parts.push(Doc::text("->"));

            if let Some(params) = n.parameters() {
                parts.push(format_node(&params, source, config));
            }

            parts.push(Doc::text(" "));
            parts.push(Doc::text(opening));

            if let Some(body) = n.body() {
                parts.push(indent_body(format_body(&body, source, config)));
                parts.push(Doc::hardline());
            }

            parts.push(Doc::text(closing));
            Doc::concat(parts)
        }

        // ── Parameters ─────────────────────────────────────────────────

        Node::BlockParametersNode { .. } => {
            let n = node.as_block_parameters_node().unwrap();
            let mut parts = Vec::new();
            if let Some(open) = n.opening_loc() {
                parts.push(Doc::text(loc_str(open, source)));
            }
            // Wrap inner params in a group so the comma+softline separators
            // stay flat unless the params themselves exceed max_width.
            let mut inner_parts = Vec::new();
            if let Some(params) = n.parameters() {
                let param_docs = format_parameters(&params, source, config);
                inner_parts.push(join_comma_separated(param_docs));
            }
            let locals: Vec<Doc> = n
                .locals()
                .iter()
                .map(|l| format_node(&l, source, config))
                .collect();
            if !locals.is_empty() {
                inner_parts.push(Doc::text("; "));
                inner_parts.push(join_comma_separated(locals));
            }
            if !inner_parts.is_empty() {
                parts.push(Doc::group(Doc::concat(inner_parts)));
            }
            if let Some(close) = n.closing_loc() {
                parts.push(Doc::text(loc_str(close, source)));
            }
            Doc::concat(parts)
        }

        Node::BlockLocalVariableNode { .. } => {
            let n = node.as_block_local_variable_node().unwrap();
            Doc::text(name_str(&n.name()))
        }

        Node::RequiredParameterNode { .. } => {
            let n = node.as_required_parameter_node().unwrap();
            Doc::text(name_str(&n.name()))
        }

        Node::OptionalParameterNode { .. } => {
            let n = node.as_optional_parameter_node().unwrap();
            let name = name_str(&n.name());
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![Doc::text(name), Doc::text(" = "), value])
        }

        Node::RestParameterNode { .. } => {
            let n = node.as_rest_parameter_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(name) = n.name() {
                Doc::concat(vec![Doc::text(op), Doc::text(name_str(&name))])
            } else {
                Doc::text(op)
            }
        }

        Node::KeywordRestParameterNode { .. } => {
            let n = node.as_keyword_rest_parameter_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(name) = n.name() {
                Doc::concat(vec![Doc::text(op), Doc::text(name_str(&name))])
            } else {
                Doc::text(op)
            }
        }

        Node::BlockParameterNode { .. } => {
            let n = node.as_block_parameter_node().unwrap();
            let op = loc_str(n.operator_loc(), source);
            if let Some(name) = n.name() {
                Doc::concat(vec![Doc::text(op), Doc::text(name_str(&name))])
            } else {
                Doc::text(op)
            }
        }

        Node::RequiredKeywordParameterNode { .. } => {
            let n = node.as_required_keyword_parameter_node().unwrap();
            Doc::text(format!("{}:", name_str(&n.name())))
        }

        Node::OptionalKeywordParameterNode { .. } => {
            let n = node.as_optional_keyword_parameter_node().unwrap();
            let name = name_str(&n.name());
            let value = format_node(&n.value(), source, config);
            Doc::concat(vec![Doc::text(format!("{}: ", name)), value])
        }

        Node::NoKeywordsParameterNode { .. } => {
            Doc::text("**nil")
        }

        Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. } => {
            Doc::Empty
        }

        // ── If / Unless ────────────────────────────────────────────────

        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();

            // Check for modifier if (postfix): `expr if cond`
            let is_modifier = if let (Some(kw_loc), Some(stmts)) = (n.if_keyword_loc(), n.statements()) {
                kw_loc.start_offset() > stmts.location().start_offset()
            } else {
                false
            };

            if is_modifier {
                let body = n.statements().unwrap();
                let body_doc = format_statements_body(&body.body(), source, config);
                let pred = format_node(&n.predicate(), source, config);
                return Doc::concat(vec![body_doc, Doc::text(" if "), pred]);
            }

            // Ternary: no if keyword in source — format children properly
            if n.if_keyword_loc().is_none() {
                let pred = format_node(&n.predicate(), source, config);
                let then_doc = if let Some(stmts) = n.statements() {
                    format_statements_body(&stmts.body(), source, config)
                } else {
                    Doc::Empty
                };
                let else_doc = if let Some(subsequent) = n.subsequent() {
                    let else_node = subsequent;
                    // ElseNode: its statements contain the else branch
                    if let Some(stmts) = else_node.as_else_node().and_then(|e| e.statements()) {
                        format_statements_body(&stmts.body(), source, config)
                    } else {
                        Doc::Empty
                    }
                } else {
                    Doc::Empty
                };
                return Doc::concat(vec![
                    pred,
                    Doc::text(" ? "),
                    then_doc,
                    Doc::text(" : "),
                    else_doc,
                ]);
            }

            let mut parts = Vec::new();
            let kw = loc_str(n.if_keyword_loc().unwrap(), source);
            let is_elsif = kw == "elsif";
            parts.push(Doc::text(kw));
            parts.push(Doc::text(" "));
            parts.push(format_node(&n.predicate(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            if let Some(subsequent) = n.subsequent() {
                parts.push(Doc::hardline());
                parts.push(format_node(&subsequent, source, config));
            }

            // Only emit `end` for the outermost `if`, not for `elsif`
            if !is_elsif && n.end_keyword_loc().is_some() {
                parts.push(Doc::hardline());
                parts.push(Doc::text("end"));
            }

            Doc::concat(parts)
        }

        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();

            let is_modifier = if let Some(stmts) = n.statements() {
                n.keyword_loc().start_offset() > stmts.location().start_offset()
            } else {
                false
            };

            if is_modifier {
                let body = n.statements().unwrap();
                let body_doc = format_statements_body(&body.body(), source, config);
                let pred = format_node(&n.predicate(), source, config);
                return Doc::concat(vec![body_doc, Doc::text(" unless "), pred]);
            }

            let mut parts = Vec::new();
            parts.push(Doc::text("unless "));
            parts.push(format_node(&n.predicate(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            if let Some(else_clause) = n.else_clause() {
                parts.push(Doc::hardline());
                parts.push(format_node(&else_clause.as_node(), source, config));
            }

            if n.end_keyword_loc().is_some() {
                parts.push(Doc::hardline());
                parts.push(Doc::text("end"));
            }

            Doc::concat(parts)
        }

        Node::ElseNode { .. } => {
            let n = node.as_else_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("else"));
            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }
            Doc::concat(parts)
        }

        // ── Case / When ────────────────────────────────────────────────

        Node::CaseNode { .. } => {
            let n = node.as_case_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("case"));
            if let Some(pred) = n.predicate() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&pred, source, config));
            }

            for condition in &n.conditions() {
                parts.push(Doc::hardline());
                parts.push(format_node(&condition, source, config));
            }

            if let Some(else_clause) = n.else_clause() {
                parts.push(Doc::hardline());
                parts.push(format_node(&else_clause.as_node(), source, config));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));
            Doc::concat(parts)
        }

        Node::CaseMatchNode { .. } => {
            let n = node.as_case_match_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("case"));
            if let Some(pred) = n.predicate() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&pred, source, config));
            }

            for condition in &n.conditions() {
                parts.push(Doc::hardline());
                parts.push(format_node(&condition, source, config));
            }

            if let Some(else_clause) = n.else_clause() {
                parts.push(Doc::hardline());
                parts.push(format_node(&else_clause.as_node(), source, config));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));
            Doc::concat(parts)
        }

        Node::WhenNode { .. } => {
            let n = node.as_when_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("when "));
            let conditions: Vec<Doc> = n
                .conditions()
                .iter()
                .map(|c| format_node(&c, source, config))
                .collect();
            parts.push(Doc::group(join_comma_separated(conditions)));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            Doc::concat(parts)
        }

        Node::InNode { .. } => {
            let n = node.as_in_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("in "));
            parts.push(format_node(&n.pattern(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            Doc::concat(parts)
        }

        // ── While / Until / For ─────────────────────────────────────────

        Node::WhileNode { .. } => {
            let n = node.as_while_node().unwrap();
            let is_modifier = if let Some(stmts) = n.statements() {
                n.keyword_loc().start_offset() > stmts.location().start_offset()
            } else {
                false
            };

            if is_modifier {
                let body = n.statements().unwrap();
                let body_doc = format_statements_body(&body.body(), source, config);
                let pred = format_node(&n.predicate(), source, config);
                return Doc::concat(vec![body_doc, Doc::text(" while "), pred]);
            }

            let mut parts = Vec::new();
            parts.push(Doc::text("while "));
            parts.push(format_node(&n.predicate(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));
            Doc::concat(parts)
        }

        Node::UntilNode { .. } => {
            let n = node.as_until_node().unwrap();
            let is_modifier = if let Some(stmts) = n.statements() {
                n.keyword_loc().start_offset() > stmts.location().start_offset()
            } else {
                false
            };

            if is_modifier {
                let body = n.statements().unwrap();
                let body_doc = format_statements_body(&body.body(), source, config);
                let pred = format_node(&n.predicate(), source, config);
                return Doc::concat(vec![body_doc, Doc::text(" until "), pred]);
            }

            let mut parts = Vec::new();
            parts.push(Doc::text("until "));
            parts.push(format_node(&n.predicate(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));
            Doc::concat(parts)
        }

        Node::ForNode { .. } => {
            let n = node.as_for_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("for "));
            parts.push(format_node(&n.index(), source, config));
            parts.push(Doc::text(" in "));
            parts.push(format_node(&n.collection(), source, config));

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            parts.push(Doc::hardline());
            parts.push(Doc::text("end"));
            Doc::concat(parts)
        }

        // ── And / Or ───────────────────────────────────────────────────

        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            let left = format_node(&n.left(), source, config);
            let op = loc_str(n.operator_loc(), source);
            let right = format_node(&n.right(), source, config);
            Doc::concat(vec![left, Doc::text(" "), Doc::text(op), Doc::text(" "), right])
        }

        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            let left = format_node(&n.left(), source, config);
            let op = loc_str(n.operator_loc(), source);
            let right = format_node(&n.right(), source, config);
            Doc::concat(vec![left, Doc::text(" "), Doc::text(op), Doc::text(" "), right])
        }

        // ── Parentheses ────────────────────────────────────────────────

        Node::ParenthesesNode { .. } => {
            let n = node.as_parentheses_node().unwrap();
            let opening = loc_str(n.opening_loc(), source);
            let closing = loc_str(n.closing_loc(), source);
            if let Some(body) = n.body() {
                Doc::concat(vec![
                    Doc::text(opening),
                    format_node(&body, source, config),
                    Doc::text(closing),
                ])
            } else {
                Doc::concat(vec![Doc::text(opening), Doc::text(closing)])
            }
        }

        // ── Begin / Rescue / Ensure ─────────────────────────────────────

        Node::BeginNode { .. } => {
            let n = node.as_begin_node().unwrap();
            let mut parts = Vec::new();

            if n.begin_keyword_loc().is_some() {
                parts.push(Doc::text("begin"));
            }

            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }

            if let Some(rescue) = n.rescue_clause() {
                parts.push(Doc::hardline());
                parts.push(format_rescue_chain(&rescue, source, config));
            }

            if let Some(else_clause) = n.else_clause() {
                parts.push(Doc::hardline());
                parts.push(format_node(&else_clause.as_node(), source, config));
            }

            if let Some(ensure) = n.ensure_clause() {
                parts.push(Doc::hardline());
                parts.push(format_node(&ensure.as_node(), source, config));
            }

            if n.end_keyword_loc().is_some() {
                parts.push(Doc::hardline());
                parts.push(Doc::text("end"));
            }

            Doc::concat(parts)
        }

        Node::RescueNode { .. } => {
            let n = node.as_rescue_node().unwrap();
            format_single_rescue(&n, source, config)
        }

        Node::RescueModifierNode { .. } => {
            let n = node.as_rescue_modifier_node().unwrap();
            let expr = format_node(&n.expression(), source, config);
            let rescue_expr = format_node(&n.rescue_expression(), source, config);
            Doc::concat(vec![expr, Doc::text(" rescue "), rescue_expr])
        }

        Node::EnsureNode { .. } => {
            let n = node.as_ensure_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("ensure"));
            if let Some(stmts) = n.statements() {
                parts.push(indent_body(
                    format_statements_body(&stmts.body(), source, config),
                ));
            }
            Doc::concat(parts)
        }

        // ── Return / Break / Next / Yield / Super ───────────────────────

        Node::ReturnNode { .. } => {
            let n = node.as_return_node().unwrap();
            let mut parts = vec![Doc::text("return")];
            if let Some(args) = n.arguments() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&args.as_node(), source, config));
            }
            Doc::concat(parts)
        }

        Node::BreakNode { .. } => {
            let n = node.as_break_node().unwrap();
            let mut parts = vec![Doc::text("break")];
            if let Some(args) = n.arguments() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&args.as_node(), source, config));
            }
            Doc::concat(parts)
        }

        Node::NextNode { .. } => {
            let n = node.as_next_node().unwrap();
            let mut parts = vec![Doc::text("next")];
            if let Some(args) = n.arguments() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&args.as_node(), source, config));
            }
            Doc::concat(parts)
        }

        Node::YieldNode { .. } => {
            let n = node.as_yield_node().unwrap();
            let mut parts = vec![Doc::text("yield")];
            if let Some(args) = n.arguments() {
                if n.lparen_loc().is_some() {
                    let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                    parts.push(format_paren_args(arg_docs));
                } else {
                    parts.push(Doc::text(" "));
                    parts.push(format_node(&args.as_node(), source, config));
                }
            } else if n.lparen_loc().is_some() {
                parts.push(Doc::text("()"));
            }
            Doc::concat(parts)
        }

        Node::SuperNode { .. } => {
            let n = node.as_super_node().unwrap();
            let mut parts = vec![Doc::text("super")];
            if let Some(args) = n.arguments() {
                if n.lparen_loc().is_some() {
                    let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
                    parts.push(format_paren_args(arg_docs));
                } else {
                    parts.push(Doc::text(" "));
                    parts.push(format_node(&args.as_node(), source, config));
                }
            } else if n.lparen_loc().is_some() {
                parts.push(Doc::text("()"));
            }
            if let Some(block) = n.block() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&block, source, config));
            }
            Doc::concat(parts)
        }

        Node::ForwardingSuperNode { .. } => {
            let n = node.as_forwarding_super_node().unwrap();
            let mut parts = vec![Doc::text("super")];
            if let Some(block) = n.block() {
                parts.push(Doc::text(" "));
                parts.push(format_node(&block.as_node(), source, config));
            }
            Doc::concat(parts)
        }

        // ── Multi-assignment ────────────────────────────────────────────

        Node::MultiWriteNode { .. } => {
            let n = node.as_multi_write_node().unwrap();
            let mut targets = Vec::new();
            for t in &n.lefts() {
                targets.push(format_node(&t, source, config));
            }
            if let Some(rest) = n.rest() {
                targets.push(format_node(&rest, source, config));
            }
            for t in &n.rights() {
                targets.push(format_node(&t, source, config));
            }
            let lhs = join_comma_separated(targets);
            let op = loc_str(n.operator_loc(), source);
            let value = format_node(&n.value(), source, config);

            if n.lparen_loc().is_some() {
                Doc::concat(vec![Doc::text("("), lhs, Doc::text(")"), Doc::text(" "), Doc::text(op), Doc::text(" "), value])
            } else {
                Doc::concat(vec![lhs, Doc::text(" "), Doc::text(op), Doc::text(" "), value])
            }
        }

        Node::MultiTargetNode { .. } => {
            let n = node.as_multi_target_node().unwrap();
            let mut targets = Vec::new();
            for t in &n.lefts() {
                targets.push(format_node(&t, source, config));
            }
            if let Some(rest) = n.rest() {
                targets.push(format_node(&rest, source, config));
            }
            for t in &n.rights() {
                targets.push(format_node(&t, source, config));
            }
            let inner = join_comma_separated(targets);
            if n.lparen_loc().is_some() {
                Doc::concat(vec![Doc::text("("), inner, Doc::text(")")])
            } else {
                inner
            }
        }

        // ── Alias / Undef / Defined? ────────────────────────────────────

        Node::AliasMethodNode { .. } => {
            let n = node.as_alias_method_node().unwrap();
            let new_name = format_node(&n.new_name(), source, config);
            let old_name = format_node(&n.old_name(), source, config);
            Doc::concat(vec![Doc::text("alias "), new_name, Doc::text(" "), old_name])
        }

        Node::AliasGlobalVariableNode { .. } => {
            let n = node.as_alias_global_variable_node().unwrap();
            let new_name = format_node(&n.new_name(), source, config);
            let old_name = format_node(&n.old_name(), source, config);
            Doc::concat(vec![Doc::text("alias "), new_name, Doc::text(" "), old_name])
        }

        Node::UndefNode { .. } => {
            let n = node.as_undef_node().unwrap();
            let names: Vec<Doc> = n.names().iter().map(|name| format_node(&name, source, config)).collect();
            Doc::concat(vec![Doc::text("undef "), join_comma_separated(names)])
        }

        Node::DefinedNode { .. } => {
            let n = node.as_defined_node().unwrap();
            let value = format_node(&n.value(), source, config);
            if n.lparen_loc().is_some() {
                Doc::concat(vec![Doc::text("defined?("), value, Doc::text(")")])
            } else {
                Doc::concat(vec![Doc::text("defined? "), value])
            }
        }

        // ── Pattern matching ────────────────────────────────────────────

        Node::MatchPredicateNode { .. } => {
            let n = node.as_match_predicate_node().unwrap();
            let value = format_node(&n.value(), source, config);
            let pattern = format_node(&n.pattern(), source, config);
            Doc::concat(vec![value, Doc::text(" in "), pattern])
        }

        Node::MatchRequiredNode { .. } => {
            let n = node.as_match_required_node().unwrap();
            let value = format_node(&n.value(), source, config);
            let pattern = format_node(&n.pattern(), source, config);
            Doc::concat(vec![value, Doc::text(" => "), pattern])
        }

        Node::MatchWriteNode { .. } => {
            let n = node.as_match_write_node().unwrap();
            format_node(&n.call().as_node(), source, config)
        }

        Node::ArrayPatternNode { .. }
        | Node::HashPatternNode { .. }
        | Node::FindPatternNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        Node::CapturePatternNode { .. } => {
            let n = node.as_capture_pattern_node().unwrap();
            let value = format_node(&n.value(), source, config);
            let target = format_node(&n.target().as_node(), source, config);
            Doc::concat(vec![value, Doc::text(" => "), target])
        }

        Node::AlternationPatternNode { .. } => {
            let n = node.as_alternation_pattern_node().unwrap();
            let left = format_node(&n.left(), source, config);
            let right = format_node(&n.right(), source, config);
            Doc::concat(vec![left, Doc::text(" | "), right])
        }

        Node::PinnedExpressionNode { .. } => {
            let n = node.as_pinned_expression_node().unwrap();
            let expr = format_node(&n.expression(), source, config);
            Doc::concat(vec![Doc::text("^("), expr, Doc::text(")")])
        }

        Node::PinnedVariableNode { .. } => {
            let n = node.as_pinned_variable_node().unwrap();
            let var = format_node(&n.variable(), source, config);
            Doc::concat(vec![Doc::text("^"), var])
        }

        // ── Flip-flop ──────────────────────────────────────────────────

        Node::FlipFlopNode { .. } => {
            Doc::text(loc_str(node.location(), source))
        }

        // ── Pre/Post execution (BEGIN/END) ──────────────────────────────

        Node::PreExecutionNode { .. } => {
            let n = node.as_pre_execution_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("BEGIN "));
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(stmts) = n.statements() {
                parts.push(indent_body(format_statements_body(&stmts.body(), source, config)));
                parts.push(Doc::hardline());
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            Doc::concat(parts)
        }

        Node::PostExecutionNode { .. } => {
            let n = node.as_post_execution_node().unwrap();
            let mut parts = Vec::new();
            parts.push(Doc::text("END "));
            parts.push(Doc::text(loc_str(n.opening_loc(), source)));
            if let Some(stmts) = n.statements() {
                parts.push(indent_body(format_statements_body(&stmts.body(), source, config)));
                parts.push(Doc::hardline());
            }
            parts.push(Doc::text(loc_str(n.closing_loc(), source)));
            Doc::concat(parts)
        }

        // ── Misc ───────────────────────────────────────────────────────

        Node::ShareableConstantNode { .. } => {
            let n = node.as_shareable_constant_node().unwrap();
            format_node(&n.write(), source, config)
        }

        Node::ImplicitNode { .. } => {
            let n = node.as_implicit_node().unwrap();
            format_node(&n.value(), source, config)
        }

        // ── Catch-all for anything we missed ────────────────────────────
        _ => {
            Doc::text(loc_str(node.location(), source))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════

/// Extract the source text for a Location as a String.
fn loc_str(loc: ruby_prism::Location<'_>, source: &[u8]) -> String {
    let bytes = &source[loc.start_offset()..loc.end_offset()];
    String::from_utf8_lossy(bytes).into_owned()
}

/// Extract a ConstantId as a String.
fn name_str(id: &ruby_prism::ConstantId<'_>) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

/// Format a simple variable write: `name = value`.
fn format_simple_write(
    name: String,
    op: String,
    value: &ruby_prism::Node<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    let value_doc = format_node(value, source, config);
    Doc::concat(vec![Doc::text(name), Doc::text(" "), Doc::text(op), Doc::text(" "), value_doc])
}

/// Format a compound write (&&=, ||=, +=, etc.).
fn format_compound_write(
    name: String,
    op: String,
    value: &ruby_prism::Node<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    let value_doc = format_node(value, source, config);
    Doc::concat(vec![Doc::text(name), Doc::text(" "), Doc::text(op), Doc::text(" "), value_doc])
}

/// Determine if a comment is a trailing comment (on the same line as preceding code)
/// by checking if there is NO newline between `prev_end` and the comment's start.
fn is_trailing_comment(source: &[u8], prev_end: usize, comment_start: usize) -> bool {
    let between = &source[prev_end..comment_start];
    !between.iter().any(|&b| b == b'\n')
}

/// Count newlines in a byte range.
fn count_newlines(source: &[u8], from: usize, to: usize) -> usize {
    source[from..to].iter().filter(|&&b| b == b'\n').count()
}

/// Format the body of a statements node — a series of statements separated
/// by hardlines. Preserves blank lines between statements. Weaves in comments.
fn format_statements_body(
    body: &ruby_prism::NodeList<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    let nodes: Vec<ruby_prism::Node<'_>> = body.iter().collect();
    if nodes.is_empty() {
        return Doc::Empty;
    }

    let mut parts: Vec<Doc> = Vec::new();

    for (i, node) in nodes.iter().enumerate() {
        let (gap_start, has_predecessor) = if i > 0 {
            (nodes[i - 1].location().end_offset(), true)
        } else {
            // For the first statement, look for leading comments from start of
            // the region. We use 0 as a sentinel; `take_comments_between` will
            // pick up any comments whose start_offset < node start.
            (0, false)
        };
        let gap_end = node.location().start_offset();

        // Collect comments that fall between the previous statement (or start) and this one.
        let comments = take_comments_between(gap_start, gap_end);

        for c in &comments {
            if has_predecessor && is_trailing_comment(source, gap_start, c.start_offset) {
                // Trailing comment on the previous statement's line.
                parts.push(Doc::line_suffix(Doc::concat(vec![
                    Doc::text(" "),
                    Doc::text(c.text.trim_end()),
                ])));
            } else {
                // Leading comment (own line) before this statement.
                if has_predecessor || !parts.is_empty() {
                    // Preserve blank lines: check gap between predecessor/previous comment and this comment.
                    let ref_offset = if parts.is_empty() { gap_start } else { gap_start };
                    let newlines = count_newlines(source, ref_offset, c.start_offset);
                    if newlines > 1 {
                        parts.push(Doc::hardline());
                    }
                    parts.push(Doc::hardline());
                }
                parts.push(Doc::text(c.text.trim_end()));
            }
        }

        // Add separator before this statement (after any leading comments).
        if i > 0 || !comments.is_empty() {
            if has_predecessor {
                // Check for blank lines between prev end and current start,
                // but only if no comments were emitted (comments handle their own spacing).
                let ref_end = if let Some(last_c) = comments.last() {
                    last_c.end_offset
                } else {
                    gap_start
                };
                let newlines = count_newlines(source, ref_end, gap_end);
                if comments.is_empty() {
                    // No comments in gap — check original blank lines
                    let total_newlines = count_newlines(source, gap_start, gap_end);
                    if total_newlines > 1 {
                        parts.push(Doc::hardline());
                        parts.push(Doc::hardline());
                    } else {
                        parts.push(Doc::hardline());
                    }
                } else {
                    // Comments were emitted — just add a hardline before the statement.
                    if newlines > 1 {
                        parts.push(Doc::hardline());
                        parts.push(Doc::hardline());
                    } else {
                        parts.push(Doc::hardline());
                    }
                }
            } else if !comments.is_empty() {
                // First statement but we have leading comments before it.
                let ref_end = comments.last().unwrap().end_offset;
                let newlines = count_newlines(source, ref_end, gap_end);
                if newlines > 1 {
                    parts.push(Doc::hardline());
                }
                parts.push(Doc::hardline());
            }
        }

        parts.push(format_node(&node, source, config));

        // Check for trailing comments on the LAST statement (after the node but
        // before the next node or end of block). We handle this for non-last
        // statements in the gap logic above; for the last statement, we check
        // for trailing comments after it.
        if i == nodes.len() - 1 {
            let node_end = node.location().end_offset();
            let trailing = take_trailing_comments_on_line(source, node_end);
            for c in &trailing {
                parts.push(Doc::line_suffix(Doc::concat(vec![
                    Doc::text(" "),
                    Doc::text(c.text.trim_end()),
                ])));
            }
        }
    }

    Doc::concat(parts)
}

/// Take comments that are trailing on the same line as the code ending at `code_end`.
fn take_trailing_comments_on_line(source: &[u8], code_end: usize) -> Vec<CommentInfo> {
    // Find the end of the current line.
    let line_end = source[code_end..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| code_end + p)
        .unwrap_or(source.len());

    take_comments_between(code_end, line_end)
}

/// Format any comments that fall in a body range (used when body is empty
/// or None). Returns Doc::Empty if no comments found.
fn format_interior_comments(from: usize, to: usize) -> Doc {
    let comments = take_comments_between(from, to);
    if comments.is_empty() {
        return Doc::Empty;
    }
    let mut parts = Vec::new();
    for (i, c) in comments.iter().enumerate() {
        if i > 0 {
            parts.push(Doc::hardline());
        }
        parts.push(Doc::text(c.text.trim_end()));
    }
    Doc::concat(parts)
}

/// Emit a hardline + indented body: the standard Wadler-Lindig pattern where
/// the newline is INSIDE the Indent so it picks up the new indent level.
fn indent_body(body: Doc) -> Doc {
    Doc::indent(Doc::concat(vec![Doc::hardline(), body]))
}

/// Format the body of a class/module/def/begin — handles StatementsNode or BeginNode.
fn format_body(body: &ruby_prism::Node<'_>, source: &[u8], config: &FormatConfig) -> Doc {
    match body {
        Node::StatementsNode { .. } => {
            let n = body.as_statements_node().unwrap();
            format_statements_body(&n.body(), source, config)
        }
        Node::BeginNode { .. } => {
            format_implicit_begin(body, source, config)
        }
        other => format_node(other, source, config),
    }
}

/// Format a BeginNode that appears as the body of a def/class (implicit begin).
fn format_implicit_begin(node: &ruby_prism::Node<'_>, source: &[u8], config: &FormatConfig) -> Doc {
    let n = node.as_begin_node().unwrap();
    let mut parts = Vec::new();

    if let Some(stmts) = n.statements() {
        parts.push(format_statements_body(&stmts.body(), source, config));
    }

    if let Some(rescue) = n.rescue_clause() {
        parts.push(Doc::hardline());
        parts.push(format_rescue_chain(&rescue, source, config));
    }

    if let Some(else_clause) = n.else_clause() {
        parts.push(Doc::hardline());
        parts.push(format_node(&else_clause.as_node(), source, config));
    }

    if let Some(ensure) = n.ensure_clause() {
        parts.push(Doc::hardline());
        parts.push(format_node(&ensure.as_node(), source, config));
    }

    Doc::concat(parts)
}

/// Format a chain of rescue clauses.
fn format_rescue_chain(rescue: &ruby_prism::RescueNode<'_>, source: &[u8], config: &FormatConfig) -> Doc {
    let mut parts = Vec::new();
    parts.push(format_single_rescue(rescue, source, config));

    if let Some(subsequent) = rescue.subsequent() {
        parts.push(Doc::hardline());
        parts.push(format_rescue_chain(&subsequent, source, config));
    }

    Doc::concat(parts)
}

/// Format a single rescue clause.
fn format_single_rescue(n: &ruby_prism::RescueNode<'_>, source: &[u8], config: &FormatConfig) -> Doc {
    let mut parts = Vec::new();
    parts.push(Doc::text("rescue"));

    let exceptions: Vec<Doc> = n
        .exceptions()
        .iter()
        .map(|e| format_node(&e, source, config))
        .collect();

    if !exceptions.is_empty() {
        parts.push(Doc::text(" "));
        parts.push(join_comma_separated(exceptions));
    }

    if let Some(reference) = n.reference() {
        parts.push(Doc::text(" => "));
        parts.push(format_node(&reference, source, config));
    }

    if let Some(stmts) = n.statements() {
        parts.push(indent_body(
            format_statements_body(&stmts.body(), source, config),
        ));
    }

    Doc::concat(parts)
}

/// Format parameters from a ParametersNode into a Vec of Doc.
fn format_parameters(
    params: &ruby_prism::ParametersNode<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Vec<Doc> {
    let mut param_docs = Vec::new();

    for p in &params.requireds() {
        param_docs.push(format_node(&p, source, config));
    }
    for p in &params.optionals() {
        param_docs.push(format_node(&p, source, config));
    }
    if let Some(rest) = params.rest() {
        param_docs.push(format_node(&rest, source, config));
    }
    for p in &params.posts() {
        param_docs.push(format_node(&p, source, config));
    }
    for p in &params.keywords() {
        param_docs.push(format_node(&p, source, config));
    }
    if let Some(kw_rest) = params.keyword_rest() {
        param_docs.push(format_node(&kw_rest, source, config));
    }
    if let Some(block) = params.block() {
        param_docs.push(format_node(&block.as_node(), source, config));
    }

    param_docs
}

/// Format arguments in parentheses with group/indent for wrapping.
fn format_paren_args(arg_docs: Vec<Doc>) -> Doc {
    if arg_docs.is_empty() {
        return Doc::text("()");
    }

    let inner = join_comma_separated(arg_docs);

    Doc::group(Doc::concat(vec![
        Doc::text("("),
        Doc::indent(Doc::concat(vec![
            Doc::softline_empty(),
            inner,
        ])),
        Doc::softline_empty(),
        Doc::text(")"),
    ]))
}

/// Check if an opening location represents a heredoc (`<<`, `<<~`, `<<-`).
fn is_heredoc_opening(opening: Option<ruby_prism::Location<'_>>, source: &[u8]) -> bool {
    if let Some(loc) = opening {
        let text = &source[loc.start_offset()..loc.end_offset()];
        text.starts_with(b"<<")
    } else {
        false
    }
}

/// Normalize string quotes to double quotes (StandardRB Style/StringLiterals).
///
/// Converts `'...'` to `"..."` unless the content contains:
/// - A literal `"` character (would need escaping)
/// - A `\` character (backslash sequences have different meaning in double quotes)
///
/// Leaves `%q{...}`, `%Q{...}`, `"..."`, heredocs, and other delimiters alone.
fn normalize_string_quotes(
    full_loc: ruby_prism::Location<'_>,
    opening_loc: Option<ruby_prism::Location<'_>>,
    content_loc: ruby_prism::Location<'_>,
    source: &[u8],
) -> Doc {
    if let Some(open_loc) = opening_loc {
        let open = &source[open_loc.start_offset()..open_loc.end_offset()];
        if open == b"'" {
            let content = &source[content_loc.start_offset()..content_loc.end_offset()];
            // Safe to convert if content has no double quotes or backslashes
            if !content.contains(&b'"') && !content.contains(&b'\\') {
                let content_str = String::from_utf8_lossy(content);
                return Doc::text(format!("\"{}\"", content_str));
            }
        }
    }
    // Fall through: keep original source verbatim
    Doc::text(loc_str(full_loc, source))
}

/// Format a StringNode heredoc. The opening tag stays inline; the body is
/// deferred via LineSuffix so it appears after the current line.
fn format_heredoc_string_node(n: ruby_prism::StringNode<'_>, source: &[u8]) -> Doc {
    let opening = loc_str(n.opening_loc().unwrap(), source);
    let content = loc_str(n.content_loc(), source);
    let closing = n.closing_loc().map(|l| loc_str(l, source)).unwrap_or_default();
    // closing typically includes a trailing newline — strip it since the
    // statement separator will add its own.
    let closing = closing.trim_end_matches('\n');
    let body = format!("\n{}{}", content, closing);
    Doc::concat(vec![
        Doc::text(opening),
        Doc::line_suffix(Doc::verbatim(body)),
    ])
}

/// Format an InterpolatedStringNode heredoc.
fn format_heredoc_interpolated_string_node(n: ruby_prism::InterpolatedStringNode<'_>, source: &[u8]) -> Doc {
    let opening = loc_str(n.opening_loc().unwrap(), source);
    // Body content spans from end of opening to start of closing.
    let body_start = n.opening_loc().unwrap().end_offset();
    let closing_loc = n.closing_loc().unwrap();
    let body_end = closing_loc.start_offset();
    let body_text = String::from_utf8_lossy(&source[body_start..body_end]).into_owned();
    let closing = loc_str(closing_loc, source);
    let closing = closing.trim_end_matches('\n');
    let full_body = format!("{}{}", body_text, closing);
    Doc::concat(vec![
        Doc::text(opening),
        Doc::line_suffix(Doc::verbatim(full_body)),
    ])
}

/// Format an XStringNode heredoc (backtick heredoc).
fn format_heredoc_xstring_node(n: ruby_prism::XStringNode<'_>, source: &[u8]) -> Doc {
    let opening = loc_str(n.opening_loc(), source);
    let content = loc_str(n.content_loc(), source);
    let closing = loc_str(n.closing_loc(), source);
    let closing = closing.trim_end_matches('\n');
    let body = format!("\n{}{}", content, closing);
    Doc::concat(vec![
        Doc::text(opening),
        Doc::line_suffix(Doc::verbatim(body)),
    ])
}

/// Format an InterpolatedXStringNode heredoc.
fn format_heredoc_interpolated_xstring_node(n: ruby_prism::InterpolatedXStringNode<'_>, source: &[u8]) -> Doc {
    let opening = loc_str(n.opening_loc(), source);
    let body_start = n.opening_loc().end_offset();
    let closing_loc = n.closing_loc();
    let body_end = closing_loc.start_offset();
    let body_text = String::from_utf8_lossy(&source[body_start..body_end]).into_owned();
    let closing = loc_str(closing_loc, source);
    let closing = closing.trim_end_matches('\n');
    let full_body = format!("{}{}", body_text, closing);
    Doc::concat(vec![
        Doc::text(opening),
        Doc::line_suffix(Doc::verbatim(full_body)),
    ])
}

/// Format `{items}` for hashes — no spaces inside braces (StandardRB).
fn format_hash_body(open: &str, close: &str, elements: Vec<Doc>) -> Doc {
    let inner = join_comma_separated(elements);

    Doc::group(Doc::concat(vec![
        Doc::text(open),
        Doc::indent(Doc::concat(vec![
            Doc::softline_empty(), // "" in flat mode (no space inside braces), newline in break mode
            inner,
        ])),
        Doc::softline_empty(), // "" in flat mode, newline in break mode
        Doc::text(close),
    ]))
}

/// Format `[items]` or `{items}` with group/indent for wrapping.
fn format_bracket_body(open: &str, close: &str, elements: Vec<Doc>) -> Doc {
    let inner = join_comma_separated(elements);

    Doc::group(Doc::concat(vec![
        Doc::text(open),
        Doc::indent(Doc::concat(vec![
            Doc::softline_empty(),
            inner,
        ])),
        Doc::softline_empty(),
        Doc::text(close),
    ]))
}

// ═══════════════════════════════════════════════════════════════════════════
// Call node formatting (the most complex part)
// ═══════════════════════════════════════════════════════════════════════════

/// Format a CallNode.
fn format_call_node(
    n: &ruby_prism::CallNode<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    let name = name_str(&n.name());

    // Unary operators
    if n.receiver().is_some()
        && n.arguments().is_none()
        && n.call_operator_loc().is_none()
        && n.opening_loc().is_none()
        && (name == "!" || name == "~" || name == "-@" || name == "+@" || name == "not")
    {
        let recv = format_node(&n.receiver().unwrap(), source, config);
        let op_name = match name.as_str() {
            "-@" => "-",
            "+@" => "+",
            "not" => return Doc::concat(vec![Doc::text("not "), recv]),
            other => other,
        };
        return Doc::concat(vec![Doc::text(op_name), recv]);
    }

    // Binary operators
    if n.receiver().is_some()
        && n.call_operator_loc().is_none()
        && n.opening_loc().is_none()
        && is_binary_operator(&name)
    {
        let recv = format_node(&n.receiver().unwrap(), source, config);
        if let Some(args) = n.arguments() {
            let arg_list: Vec<ruby_prism::Node<'_>> = args.arguments().iter().collect();
            if arg_list.len() == 1 {
                let rhs = format_node(&arg_list[0], source, config);
                return Doc::concat(vec![recv, Doc::text(" "), Doc::text(&name), Doc::text(" "), rhs]);
            }
        }
    }

    // aref: a[b]
    if name == "[]" && n.opening_loc().is_some() {
        let mut parts = Vec::new();
        if let Some(recv) = n.receiver() {
            parts.push(format_node(&recv, source, config));
        }
        parts.push(Doc::text("["));
        if let Some(args) = n.arguments() {
            let arg_docs: Vec<Doc> = args.arguments().iter().map(|a| format_node(&a, source, config)).collect();
            parts.push(join_comma_separated(arg_docs));
        }
        parts.push(Doc::text("]"));
        if let Some(block) = n.block() {
            parts.push(Doc::text(" "));
            parts.push(format_node(&block, source, config));
        }
        return Doc::concat(parts);
    }

    // aset: a[b] = c
    if name == "[]=" && n.opening_loc().is_some() {
        let mut parts = Vec::new();
        if let Some(recv) = n.receiver() {
            parts.push(format_node(&recv, source, config));
        }
        parts.push(Doc::text("["));
        if let Some(args) = n.arguments() {
            let arg_list: Vec<ruby_prism::Node<'_>> = args.arguments().iter().collect();
            if arg_list.len() > 1 {
                let index_args: Vec<Doc> = arg_list[..arg_list.len() - 1]
                    .iter()
                    .map(|a| format_node(a, source, config))
                    .collect();
                parts.push(join_comma_separated(index_args));
                parts.push(Doc::text("] = "));
                parts.push(format_node(&arg_list[arg_list.len() - 1], source, config));
            } else {
                parts.push(Doc::text("] = "));
                if !arg_list.is_empty() {
                    parts.push(format_node(&arg_list[0], source, config));
                }
            }
        } else {
            parts.push(Doc::text("]"));
        }
        return Doc::concat(parts);
    }

    // Method chains (3+ segments: a.b.c or more)
    if n.receiver().is_some() && n.call_operator_loc().is_some() {
        // Check if this is a multi-segment chain (receiver is also a dotted call)
        let is_chain = n.receiver().and_then(|r| {
            r.as_call_node().filter(|cn| cn.call_operator_loc().is_some())
        }).is_some();

        if is_chain {
            return format_method_chain(n, source, config);
        }
        // Single-segment "chain" (e.g. Mailer.deliver(...)) — fall through to
        // regular call formatting below.
    }

    // Regular method call
    let mut parts = Vec::new();

    if let Some(recv) = n.receiver() {
        parts.push(format_node(&recv, source, config));
        if let Some(op) = n.call_operator_loc() {
            parts.push(Doc::text(loc_str(op, source)));
        }
    }

    if let Some(msg_loc) = n.message_loc() {
        parts.push(Doc::text(loc_str(msg_loc, source)));
    }

    // Determine if the block is a BlockArgumentNode (&:role, &block) — these go
    // inside the parentheses as an argument — vs a BlockNode (do..end, {}) which
    // goes after the call.
    let block = n.block();
    let is_block_arg = block.as_ref().map_or(false, |b| b.as_block_argument_node().is_some());
    let block_arg_doc = if is_block_arg {
        Some(format_node(block.as_ref().unwrap(), source, config))
    } else {
        None
    };

    if let Some(args) = n.arguments() {
        let mut arg_docs: Vec<Doc> = args
            .arguments()
            .iter()
            .map(|a| format_node(&a, source, config))
            .collect();

        // Append block argument (&:role) to the argument list
        if let Some(ba) = block_arg_doc {
            arg_docs.push(ba);
        }

        if n.opening_loc().is_some() {
            parts.push(format_paren_args(arg_docs));
        } else {
            parts.push(Doc::text(" "));
            parts.push(Doc::group(join_comma_separated(arg_docs)));
        }
    } else if let Some(ba) = block_arg_doc {
        // No regular arguments, but there's a block argument like group_by(&:role)
        if n.opening_loc().is_some() {
            parts.push(format_paren_args(vec![ba]));
        } else {
            parts.push(Doc::text(" "));
            parts.push(ba);
        }
    } else if n.opening_loc().is_some() {
        parts.push(Doc::text("()"));
    }

    if !is_block_arg {
        if let Some(ref blk) = block {
            parts.push(Doc::text(" "));
            parts.push(format_node(blk, source, config));
        }
    }

    Doc::concat(parts)
}

fn is_binary_operator(name: &str) -> bool {
    matches!(
        name,
        "+"  | "-"  | "*"  | "/"  | "%"  | "**"
        | "==" | "!=" | "<"  | ">"  | "<=" | ">="
        | "<=>" | "=~" | "!~"
        | "&"  | "|"  | "^"  | "<<" | ">>"
        | "==="
    )
}

/// A segment in a method chain.
struct ChainSegment {
    /// The concatenated call operators and method names for this segment.
    /// For merged bare methods like `.where.not`, this contains multiple ops+names.
    prefix_parts: Vec<Doc>,
    args: Option<Doc>,
    block: Option<Doc>,
}

/// Format a method chain: `obj.foo.bar.baz(args)`.
fn format_method_chain(
    node: &ruby_prism::CallNode<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    // A raw segment before merging bare methods.
    struct RawSegment {
        call_op: String,
        name: String,
        args: Option<Doc>,
        block: Option<Doc>,
    }

    // Extract segment data from a CallNode
    fn extract_raw_segment(
        cn: &ruby_prism::CallNode<'_>,
        source: &[u8],
        config: &FormatConfig,
    ) -> RawSegment {
        let name = name_str(&cn.name());
        let call_op = cn
            .call_operator_loc()
            .map(|l| loc_str(l, source))
            .unwrap_or_default();

        // Determine if block is a &:sym style argument or a do..end/{} block
        let block_node = cn.block();
        let is_block_arg = block_node.as_ref().map_or(false, |b| b.as_block_argument_node().is_some());
        let block_arg_doc = if is_block_arg {
            Some(format_node(block_node.as_ref().unwrap(), source, config))
        } else {
            None
        };

        let args = if let Some(args) = cn.arguments() {
            let mut arg_docs: Vec<Doc> = args
                .arguments()
                .iter()
                .map(|a| format_node(&a, source, config))
                .collect();
            if let Some(ba) = block_arg_doc {
                arg_docs.push(ba);
            }
            if cn.opening_loc().is_some() {
                Some(format_paren_args(arg_docs))
            } else {
                Some(Doc::concat(vec![
                    Doc::text(" "),
                    Doc::group(join_comma_separated(arg_docs)),
                ]))
            }
        } else if let Some(ba) = block_arg_doc {
            if cn.opening_loc().is_some() {
                Some(format_paren_args(vec![ba]))
            } else {
                Some(Doc::concat(vec![Doc::text(" "), ba]))
            }
        } else if cn.opening_loc().is_some() {
            Some(Doc::text("()"))
        } else {
            None
        };

        let block = if !is_block_arg {
            if let Some(b) = block_node {
                Some(Doc::concat(vec![Doc::text(" "), format_node(&b, source, config)]))
            } else {
                None
            }
        } else {
            None
        };

        RawSegment {
            call_op,
            name,
            args,
            block,
        }
    }

    // Walk the chain collecting raw segments. We start from the outermost call
    // and walk inward through receivers.
    let mut raw_segments: Vec<RawSegment> = Vec::new();
    raw_segments.push(extract_raw_segment(node, source, config));

    let receiver_doc;
    // Walk through receiver chain
    let mut recv_opt = node.receiver();
    loop {
        match recv_opt {
            Some(ref recv) => {
                if let Some(call_node) = recv.as_call_node() {
                    if call_node.call_operator_loc().is_some() {
                        raw_segments.push(extract_raw_segment(&call_node, source, config));
                        recv_opt = call_node.receiver();
                        continue;
                    }
                }
                receiver_doc = format_node(recv, source, config);
                break;
            }
            None => {
                receiver_doc = Doc::Empty;
                break;
            }
        }
    }

    raw_segments.reverse();

    // Each raw segment becomes its own chain segment — no merging of bare methods.
    let segments: Vec<ChainSegment> = raw_segments
        .into_iter()
        .map(|raw| ChainSegment {
            prefix_parts: vec![Doc::text(&raw.call_op), Doc::text(&raw.name)],
            args: raw.args,
            block: raw.block,
        })
        .collect();

    // Build a doc for a single segment (prefix + args + block)
    fn segment_doc(seg: &ChainSegment) -> Doc {
        let mut parts: Vec<Doc> = seg.prefix_parts.clone();
        if let Some(ref args) = seg.args {
            parts.push(args.clone());
        }
        if let Some(ref block) = seg.block {
            parts.push(block.clone());
        }
        Doc::concat(parts)
    }

    // Determine whether to force-break: chains with 4+ segments always break.
    let force_break = segments.len() >= 4;

    // Build flat version: receiver + all segments inline
    let mut flat_parts = Vec::new();
    for seg in &segments {
        flat_parts.push(segment_doc(seg));
    }
    let flat_chain = Doc::concat(flat_parts);

    // Build broken version: receiver on its own line, each segment on a new
    // line indented by one level.
    let mut break_parts = Vec::new();
    for seg in &segments {
        break_parts.push(Doc::hardline());
        break_parts.push(segment_doc(seg));
    }
    let break_chain = Doc::indent(Doc::concat(break_parts));

    if force_break {
        // Force the broken layout regardless of width.
        Doc::concat(vec![receiver_doc, break_chain])
    } else if segments.len() == 2 {
        // 2-segment chains: use BestOf to let the printer choose.
        // The flat variant allows inner groups (like paren-args) to break
        // independently while the chain itself stays flat.
        let flat_variant = Doc::concat(vec![receiver_doc.clone(), flat_chain]);
        let break_variant = Doc::concat(vec![receiver_doc, break_chain]);
        Doc::best_of(vec![flat_variant, break_variant])
    } else {
        // 3-segment chains: use standard Group(IfBreak) so the chain stays
        // flat only if EVERYTHING fits on one line. If not, the chain breaks
        // and each segment gets its own line.
        Doc::concat(vec![
            receiver_doc,
            Doc::group(Doc::if_break(break_chain, flat_chain)),
        ])
    }
}

/// Format a block node (do..end or { }).
fn format_block_node(
    n: &ruby_prism::BlockNode<'_>,
    source: &[u8],
    config: &FormatConfig,
) -> Doc {
    let opening = loc_str(n.opening_loc(), source);
    let is_brace = opening == "{";

    let params_doc = if let Some(params) = n.parameters() {
        Some(format_node(&params, source, config))
    } else {
        None
    };

    if is_brace {
        let mut parts = Vec::new();
        parts.push(Doc::text("{"));
        if let Some(p) = params_doc {
            parts.push(Doc::text(" "));
            parts.push(p);
        }
        if let Some(body) = n.body() {
            parts.push(Doc::text(" "));
            parts.push(format_body(&body, source, config));
            parts.push(Doc::text(" "));
        }
        parts.push(Doc::text("}"));
        Doc::group(Doc::concat(parts))
    } else {
        let mut parts = Vec::new();
        parts.push(Doc::text("do"));
        if let Some(p) = params_doc {
            parts.push(Doc::text(" "));
            parts.push(p);
        }

        if let Some(body) = n.body() {
            parts.push(indent_body(format_body(&body, source, config)));
        }

        parts.push(Doc::hardline());
        parts.push(Doc::text("end"));
        Doc::concat(parts)
    }
}
