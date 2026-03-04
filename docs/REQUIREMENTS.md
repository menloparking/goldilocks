# Goldilocks — Project Requirements

## Goal

Goldilocks is a Ruby code formatter (beautifier) that reformats Ruby source code
to fit within a configurable column width. It is built on the `ruby-prism` Rust
crate and uses Wadler-Lindig pretty-printing.

## StandardRB Compatibility (Primary Constraint)

Goldilocks output MUST be a **fixed point under StandardRB** — that is, running
`standardrb --fix` on Goldilocks-formatted code should produce zero changes.
This is the single most important correctness property.

Equivalently: `standardrb(goldilocks(input)) == goldilocks(input)`

If this property does not hold for some construct, Goldilocks must be changed
to match StandardRB's expectations, since StandardRB defines "standard Ruby."

### StandardRB Style Rules (as of v1.54.0)

These are the rules StandardRB enforces that Goldilocks must comply with:

1. **String literals: double quotes**
   - StandardRB enforces `Style/StringLiterals` → double quotes by default
   - `'hello'` → `"hello"`
   - Goldilocks must emit double-quoted strings (unless the string contains
     double quotes, in which case single quotes are acceptable)

2. **Line length: 120 columns** (NOT 100)
   - StandardRB's `Layout/LineLength` max is **120**, not 100
   - However, Goldilocks targets **100 columns** as its formatting width
   - This is stricter than StandardRB, which is fine — StandardRB will not
     re-wrap our already-short lines
   - But Goldilocks must never produce output that StandardRB would re-wrap,
     which means understanding what StandardRB considers "too long" (120)

3. **Hash brace spacing: NO spaces inside braces**
   - StandardRB enforces `Layout/SpaceInsideHashLiteralBraces` → no_space
   - `{ k: v }` → `{k: v}`
   - Goldilocks must emit `{k: v}` not `{ k: v }`
   - EXCEPTION: hash-rocket syntax retains spaces around `=>`: `{"k" => v}`

4. **Method chain indentation: align with receiver**
   - StandardRB enforces `Layout/MultilineMethodCallIndentation` → aligned
   - ```ruby
     # StandardRB wants:
     current_user
       .orders          # WRONG — StandardRB wants alignment with receiver
     current_user
     .orders            # Also wrong
     current_user
       .orders          # 2-space indent from line start — need to verify
     ```
   - NEEDS INVESTIGATION: StandardRB flagged our 2-space chain indent as
     wrong, wanting alignment with `current_user` on the preceding line.
     This may mean chains should use alignment, not fixed indent.

5. **Frozen string literal comment**: StandardRB warns about missing
   `# frozen_string_literal: true` but this is informational only (not
   auto-corrected in a meaningful way for us — our formatter preserves
   existing comments but doesn't add new ones)

6. **Block style**: `Style/SymbolProc` — `each { |u| u.activate! }` should
   be `each(&:activate!)` — this is a SEMANTIC transform, not a formatting
   one. Goldilocks should not perform semantic transforms. This is acceptable
   as a StandardRB-only concern.

## Column Width

- **Goldilocks target**: 100 columns (configurable via `max_width`)
- **StandardRB limit**: 120 columns
- Lines up to 100 characters are always acceptable to both
- Lines between 101-120: acceptable to StandardRB, but Goldilocks should
  break these (our target is tighter)
- Lines over 120: StandardRB flags these, and they are NOT auto-correctable
  by StandardRB — so Goldilocks must ensure its output never exceeds 120
  even when it cannot fit within 100 (e.g., long string literals)

### Column Counting

- "100 columns" means the line contains at most 100 characters
- A line of exactly 100 characters is acceptable
- A line of 101 characters must be broken (if breakable)
- Tab characters: not applicable (Ruby standard style uses spaces only)

## Architecture

### Three-Stage Pipeline

1. **AST → IR** (`formatter.rs`): Converts Prism AST nodes into a `Doc` IR
2. **IR**: An algebraic data type with constructors: `Text`, `Hardline`,
   `Softline`, `Concat`, `Indent`, `Align`, `Group`, `IfBreak`,
   `LineSuffix`, `Verbatim`, `Empty`, `BestOf`
3. **IR → String** (`printer.rs`): Wadler-Lindig stack-based printer that
   decides flat-vs-break for each Group based on remaining line width

### Key IR Constructs

- `Group(doc)`: Try to print `doc` flat; if it doesn't fit, break it
- `IfBreak(break_doc, flat_doc)`: Choose between two forms based on
  whether the enclosing group broke
- `BestOf(Vec<Doc>)`: Try multiple variants, pick the first that fits
  within max_width. Falls back to the last variant.
- `Indent(doc)`: Add `indent_width` (2) spaces to the current indent level
- `LineSuffix(doc)`: Defer `doc` to the end of the current line (for
  trailing comments)
- `Verbatim(string)`: Emit string exactly as-is (for heredocs, etc.)

## Formatting Rules by Construct

### Method Calls with Arguments

- If the call fits on one line: keep it flat
- If it doesn't fit: break after opening paren, indent args 2 spaces
  from line start, closing paren on its own line at original indent
- ```ruby
  # Flat (fits):
  User.create(name: "Alice", email: "alice@example.com")

  # Broken (doesn't fit at current indent):
  User.create(
    first_name: "Alice",
    last_name: "Wonderland",
    email: "alice@wonderland.example.com"
  )
  ```

### Method Chains

- **2 segments**: Use BestOf to try flat first, break if needed
- **3 segments**: Use Group/IfBreak
- **4+ segments**: Always break (one call per line)
- Broken chains indent 2 spaces from the line containing the receiver
  (NEEDS VERIFICATION against StandardRB alignment rule)

### Hashes

- Inline: `{name: "John", age: 30}` (no spaces inside braces)
- Broken:
  ```ruby
  {
    name: "John",
    age: 30
  }
  ```

### Arrays

- Inline: `[1, 2, 3]` (no spaces inside brackets)
- Broken:
  ```ruby
  [
    1,
    2,
    3
  ]
  ```

### Strings

- Always use double quotes (StandardRB requirement)
- Preserve string content exactly
- Heredocs: preserve verbatim (opening tag inline, body deferred via
  LineSuffix)
- Long strings that exceed the column limit: leave as-is (cannot be
  broken without changing semantics)

### Conditionals

- `if`/`unless`/`while`/`until` with multi-line bodies: standard block form
- Short conditionals may use modifier form (post-condition)
- Ternary: preserve if it fits

### Blocks

- Short blocks: `{ |x| expr }` (braces, single line)
- Long blocks: `do |x| ... end` (do/end, multi-line)
- Block arguments: `(&:method_name)` inside parens

### Word Arrays

- `%w[word1 word2 word3]` — normalized spacing
- `%i[sym1 sym2 sym3]` — normalized spacing

### Comments

- Leading comments: preserved above the node
- Trailing comments: preserved at end of line via LineSuffix
- Comments are never reformatted or moved

## Testing Requirements

### Fixture Tests

Each fixture must include:
1. An input `.rb` file with Ruby code in various states of formatting
2. An `.expected.rb` file with the exact expected Goldilocks output
3. The expected output MUST be a fixed point under StandardRB
   (i.e., `standardrb --fix expected.rb` produces no changes)

### Boundary Sensitivity Tests

For each construct type that can be broken across lines, fixtures MUST
include pairs of examples that differ by 1-2 characters, such that:
- One example fits within 100 columns (stays on one line)
- The other exceeds 100 columns (gets broken)

This tests the off-by-one correctness of the column-counting logic.

Constructs with boundary tests (fixture 11):
- [x] Method calls with arguments (2 pairs)
- [x] Hash literals (1 pair)
- [x] Array literals (1 pair)
- [x] 3-segment method chains (1 pair)
- [x] 4+ segment chains (force-break, 1 case)
- [x] Indented method calls (2-space, 1 pair)
- [x] Deeply indented method calls (4-space, 1 pair)
- [ ] Conditionals (if/unless modifier form vs block form)
- [ ] String assignments
- [ ] Block calls with arguments

### Round-Trip Stability

The following property must hold for all valid Ruby input:
```
goldilocks(standardrb(goldilocks(input))) == goldilocks(input)
```

Ideally, the stronger property holds:
```
standardrb(goldilocks(input)) == goldilocks(input)
```

## Non-Goals

- Goldilocks does NOT perform semantic transforms (e.g., `each { |x| x.foo }`
  → `each(&:foo)`)
- Goldilocks does NOT add comments (e.g., `# frozen_string_literal: true`)
- Goldilocks does NOT reorder code (e.g., alphabetize methods)
- Goldilocks does NOT enforce naming conventions
