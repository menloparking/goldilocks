# Goldilocks Design Notes

## The Indentation-Width Tension

The central design challenge of a Ruby formatter that must produce
**standard-Ruby-conformant output** (2-space indentation, ≤100 columns):

### The Problem

The Wadler-Lindig pretty-printing algorithm decides whether a group "fits"
on the current line by measuring remaining width. But Ruby's structural
indentation means the same expression appears at different indent depths
depending on where it lives:

```ruby
# At top level (indent=0): 93 chars → fits on one line
User.create(first_name: 'Alice', last_name: 'Wonderland', email: 'alice@wonderland.example.com')

# Inside a deeply nested class (indent=8): 93 + 8 = 101 chars → must break!
class Foo
  module Bar
    class Baz
      def go
        User.create(first_name: 'Alice', last_name: 'Wonderland', email: 'alice@wonderland.example.com')
        # ^^^^^^^^^ 101 chars, over the limit
      end
    end
  end
end
```

The formatter builds the Doc IR in a single pass over the AST, but the
printer decides flat-vs-break based on the *current indent + content width*.
This is actually fine — the printer naturally handles this because:

1. When the printer encounters a `Group`, it checks `fits()` against
   `max_width - current_position`, where `current_position` already
   accounts for indentation.
2. The same `Group` will go flat at indent=0 but break at indent=8.

**So the basic algorithm is correct.** The issue is NOT that we fail to
account for indentation — Wadler-Lindig handles this automatically. The
issue is subtler.

### Where It Gets Tricky

**Re-breaking can cascade.** When a group breaks:
- Its softlines become newlines+indent
- The new indentation pushes *nested* groups further right
- Those nested groups may now also need to break
- This can cascade through multiple levels

This is actually **desirable behavior** — it's exactly what we want. A
call like `Mailer.deliver(to: x, subject: y, body: z)` should stay on one
line at indent=0, but when it's inside a `def` inside a `class`, the extra
8 chars of indentation should cause it to break.

The Wadler-Lindig algorithm handles this correctly because it processes
the document top-down: each Group is evaluated with the *actual* current
position, which includes all accumulated indentation. No iteration needed.

### What About Post-Hoc Verification?

One might worry: "what if the formatter's output exceeds 100 columns
somewhere?" This could happen only if:

1. **Atomic tokens exceed the width** — a string literal, heredoc content,
   or identifier is simply longer than 100 chars. We can't break these
   (strings are preserved verbatim). This is acceptable and matches what
   rubocop does (it exempts long strings).

2. **The fits() check has a bug** — e.g., not accounting for indent
   correctly in the measurement. We should add a debug mode that scans
   the output and warns about any lines >100 chars (excluding strings
   and comments).

3. **Comments push lines over** — a trailing comment on an already-long
   line. Since we preserve comments as `LineSuffix`, they're appended
   after the content. If content is 80 chars and the comment is 30 chars,
   the line is 110 chars. This matches rubocop behavior (it only measures
   the code portion, or has a separate comment-length rule).

### Design Decision: No Iteration Needed

Unlike some formatters (e.g., Black for Python), we do NOT need an
iterative "format, check, re-format" loop because:

- Wadler-Lindig is a single-pass algorithm that naturally accounts for
  indentation depth during the fits() check.
- We are not doing transformations that change structure (like converting
  ternaries to if/else based on length) — those are done in the formatter
  based on the AST, not based on the printed width.
- The only case where width could surprise us is truly atomic content
  (long strings), which we explicitly don't break.

### Spaces in `format_paren_args` — 2 Not 4

When breaking arguments in parentheses, Standard Ruby indents 2 spaces
from the *start of the line containing the opening paren*, NOT aligned
to the paren position:

```ruby
# CORRECT (Standard Ruby):
User.create(
  first_name: 'Alice',    # 2 spaces from line start
  last_name: 'Bob'
)

# WRONG (paren-aligned):
User.create(
            first_name: 'Alice',    # aligned to '(' — wastes space
            last_name: 'Bob'
            )
```

This is crucial: paren-alignment pushes content rightward, especially
in chains (`obj.method_name(` already consumed 15+ chars), making
100-column overflows much more likely. Standard Ruby's 2-space indent
keeps things compact and predictable.

Our `format_paren_args` uses `Doc::indent(Doc::softline_empty() + items)`
which is correct — `Doc::indent` adds `indent_width` (2) spaces, not
alignment to the paren.

### Comment Handling Strategy

Comments are not part of the Prism AST nodes — they're returned
separately via `parse_result.comments()`. We must weave them into the
output by:

1. Collecting all comments with their source positions
2. For each AST node, checking if any comments fall between the previous
   node's end and this node's start (leading comments) or on the same
   line after the node (trailing comments)
3. Leading comments → `Doc::hardline()` + `Doc::text("# ...")`
4. Trailing comments → `Doc::line_suffix(Doc::text("  # ..."))`

This is the standard approach used by Prettier and similar formatters.
