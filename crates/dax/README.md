# gridcraft-dax

The DAX language for GridCraft: a tokenizer, a Pratt parser, a syntax tree, a canonical printer
and the aggregation function catalog with static call validation. This crate is the foundation
for later DAX PRs (data types and statements). It has no evaluator and no engine dependency.

```rust
use gridcraft_dax::{parse_expression, print_expression};

let expr = parse_expression("1 + 2 * 3")?;
assert_eq!(print_expression(&expr), "1 + 2 * 3");

let query = gridcraft_dax::parse_query("EVALUATE SUM('Sales'[Amount])")?;
assert_eq!(gridcraft_dax::print_query(&query), "EVALUATE SUM('Sales'[Amount])");
# Ok::<(), gridcraft_dax::ParseError>(())
```

## Public API

| Item | Meaning |
| --- | --- |
| `parse_expression(&str) -> Result<Expr, ParseError>` | Parse one expression. One leading `=` (as typed in some editors) is accepted and ignored. |
| `print_expression(&Expr) -> String` | Canonical text. |
| `parse_query(&str) -> Result<Query, ParseError>` | Parse `EVALUATE <expression>` (only statement form in this PR). |
| `print_query(&Query) -> String` | Canonical query text. |
| `tokenize(&str) -> Result<Vec<Token>, LexError>` | Tokens with byte spans, including `Eof`. |
| `ParseError { message, span }` / `LexError { message, span }` | `Display + std::error::Error`; the lexer error converts with `From`. |
| `ast::{Expr, ExprKind, Literal, Name, NameRef, NameStyle, Call, UnaryOp, BinaryOp, Span, Query}` | Syntax tree. |
| `functions::{catalog, lookup}` | The 22 aggregation functions, looked up case-insensitively. |
| `functions::{FunctionSpec, FunctionCategory, ParamKind, ReturnType, BlankPolicy, FunctionFlags, Parameter, Overload}` | Catalog record types. |
| `validate_call(&Call) -> Vec<ValidationError>` | Arity/kind checks for one call; empty for valid or unknown functions. |
| `validate_expression(&Expr) -> Vec<ValidationError>` | Walks the tree and reports every invalid known-function call with its span. |
| `MAX_INPUT_LEN` (8192 bytes) / `MAX_DEPTH` (256) | Limits enforced with errors. |

### Syntax tree

```text
Expr { kind: ExprKind, span: Span }
ExprKind =
    Literal(Literal)                     Number(f64) | String(String) | Boolean(bool)
  | Name(NameRef)                        table: Option<Name>, name: Name
  | Call(Call)                           name: Name, args: Vec<Expr>
  | Unary { op, expr }                   op: UnaryOp (Plus | Neg | Not)
  | Binary { op, left, right }            op: BinaryOp
  | Table(Vec<Expr>)                     { value, ... }
  | Tuple(Vec<Expr>)                     (value, ...)
```

`Span` is a half-open byte range into the source. `Expr::walk` visits nodes depth-first with an
explicit stack. `Expr` equality compares `ExprKind` only: spans are ignored, so
`parse(print(e)) == e` for every expression the parser produces.

`Name` keeps the source spelling and quoting style (`Bare`, `Quoted`, `Bracketed`); the printer
reproduces that style. A `NameRef` is a table-qualified reference (`'Sales'[Amount]`) or an
unqualified name (`[Amount]`, `Total`, `'Sales'`). `Call` also keeps the spelling (`Sum` stays
`Sum`).

Parentheses are grouping, not AST nodes. A parenthesised comma list is a `Tuple`; inside
`{ ... }` those tuples are the rows of a `Table`.

## Grammar

```text
expression   := prefix (binary_operator prefix)*        -- precedence-climbing, see the table
prefix       := "+" prefix | "-" prefix | NOT prefix | primary
primary      := NUMBER | STRING | TRUE | FALSE
              | name
              | name "(" [ expression ("," expression)* ] ")"
              | table-qualified-name
              | "(" expression ")"
              | "(" expression "," expression ("," expression)* ")"
              | "{" [ expression ("," expression)* ] "}"
name         := IDENT | "[" column "]" | "'" table "'"
qualified    := (IDENT | "'" table "'") "[" column "]"
query        := EVALUATE expression
```

The shape above is illustrative: the Pratt parser applies the precedence and associativity
table below rather than a fixed grammar cascade.

`*` is not a wildcard token: it is only the multiplication operator.

### Case and escapes

- Keywords (`NOT`, `IN`, `EVALUATE`, `TRUE`, `FALSE`) are case-insensitive. `TRUE`/`FALSE` are
  literals unless followed by `(` (then they are calls to a function of that name).
- Bare identifiers keep their spelling. Function names may contain `.` (`STDEV.P`).
- Single-quoted table identifiers double an embedded quote: `'It''s'`.
- Bracketed columns double an embedded `]`: `[A]]B]`.
- String literals double an embedded quote: `"a""b"`.
- Number literals: `1`, `1.5`, `.5`, `1.`, and exponents `1e3`, `1.5E-3`. Values must be finite.
- Comments: `// line`, `-- line`, and `/* block */` (blocks do not nest).

## Operators

Precedence from the DAX Guide operator table (tighter first); equal precedence evaluates left to
right, except `^`:

| Level | Operators | Associativity |
| --- | --- | --- |
| 1 | grouping `()`, function calls, `IN` | — (`IN` is an infix operator) |
| 2 | `^` | right |
| 3 | unary `+`, `-` | prefix |
| 4 | `*`, `/` | left |
| 5 | binary `+`, `-` | left |
| 6 | `&` | left |
| 7 | `=`, `==`, `<>`, `<`, `>`, `<=`, `>=` | left |
| 8 | `NOT` | prefix |
| 9 | `&&` | left |
| 10 | `\|\|` | left |

`=` coerces types at evaluation time; `==` is strict equality. This crate only records which
operator was written. `IN` takes a list (`expr IN { v1, v2 }`) or a table expression
(`expr IN VALUES(T[Col])`).

Excel evaluates `2^3^2` as `512` (`2^(3^2)`), so `^` is right-associative even though the generic
DAX Guide note says operators of equal precedence go left to right. Unary minus binds looser than
`^`: `-2^2` parses as `-(2^2)`.

## Limits, errors and printing

Input is capped at 8192 bytes and expression nesting at 256 levels; both produce an error, never a
stack overflow. The printer and `Expr::walk` use explicit stacks, and the parser's binary loop is
iterative for left-associated chains.

Errors carry `message` and a byte `span`; `Display` includes both. The canonical printer uses
spaces around binary operators, `, ` between arguments/items, and adds parentheses only where
precedence or associativity requires them. Re-parsing canonical text yields the same AST.

## Aggregation function catalog

`functions::catalog()` returns the 22 functions of
[dax.guide/functions/aggregation](https://dax.guide/functions/aggregation/), in that order.
`functions::lookup(name)` is case-insensitive. Each record carries its descriptive signature
(`params`, with `optional` markers), exact `overloads`, `return_type`, `flags`
(`iterator`, `not_recommended`, `excel_compatible`, `direct_query_limited`), `sugar_of`,
`blank_policy`, `description` and `examples`. Every example in the catalog parses with
`parse_expression` and validates.

| Function | Arity | Iterator | Sugar of | Blank | Excel | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `APPROXIMATEDISTINCTCOUNT` | 1 | no | — | BLANK | no | HyperLogLog / DirectQuery pushdown; column reference only. |
| `AVERAGE` | 1 | no | `AVERAGEX` | unspecified | yes | Ignores blanks, includes zero; sources disagree on the empty/all-blank result. |
| `AVERAGEA` | 1 | no | — | unspecified | yes | Booleans are 1/0 and any text is 0; dax.guide marks it not recommended. |
| `AVERAGEX` | 2 | yes | — | BLANK | yes | Row context; blanks ignored, zeros included; empty set observed BLANK. |
| `COUNT` | 1 | no | `COUNTX` | BLANK | yes | Counts numbers, dates and strings; empty strings count, booleans do not. |
| `COUNTA` | 1 | no | `COUNTAX` | BLANK | yes | Counts non-blank values including booleans. |
| `COUNTAX` | 2 | yes | — | BLANK | yes | Row context; booleans count. |
| `COUNTBLANK` | 1 | no | — | unspecified | yes | Empty string counts as blank; dax.guide says never 0, Microsoft Learn says 0 when rows exist. |
| `COUNTROWS` | 0–1 | no | — | BLANK | yes\* | \*The zero-argument form is rejected by Excel's embedded engine; pass a table there. |
| `COUNTX` | 2 | yes | — | BLANK | yes | Row context; booleans are not counted. |
| `DISTINCTCOUNT` | 1 | no | — | BLANK | yes | BLANK is one distinct value; totals are non-additive. |
| `DISTINCTCOUNTNOBLANK` | 1 | no | `DISTINCTCOUNT` | unspecified | no | Excludes BLANK; rejected by Excel's embedded engine. |
| `MAX` | 1–2 | no | `MAXX`\* | unspecified | yes | \*Only the one-argument column overload is sugar. Two scalars treat BLANK as significant per dax.guide, as 0 per Microsoft Learn. |
| `MAXA` | 1 | no | — | unspecified | yes | TRUE/FALSE become 1/0; empty result conflicts across sources. |
| `MAXX` | 2–3 | yes | — | unspecified | yes | Optional third `Variant` compares mixed types per `ORDER BY DESC`. |
| `MIN` | 1–2 | no | `MINX`\* | unspecified | yes | \*Only the one-argument column overload is sugar. Two scalars have the same BLANK conflict as `MAX`. |
| `MINA` | 1 | no | — | unspecified | yes | TRUE/FALSE become 1/0; empty result conflicts across sources. |
| `MINX` | 2–3 | yes | — | unspecified | yes | Optional third `Variant` compares mixed types per `ORDER BY ASC`. |
| `PRODUCT` | 1 | no | `PRODUCTX` | BLANK | yes | Multiplies numeric cells, ignoring blanks, logical values and text. |
| `PRODUCTX` | 2 | yes | — | BLANK | yes | Row context; ignores blank results, unlike the `*` operator. |
| `SUM` | 1 | no | `SUMX` | BLANK | yes | Adds numeric cells, ignoring blanks, logical values and text. |
| `SUMX` | 2 | yes | — | BLANK | yes | Row context; blanks, logical values and text ignored. |

`AVERAGEA`, `MAXA` and `MINA` use the X function internally but are not pure sugar: their
boolean/text coercions differ, so `sugar_of` is `None`. `direct_query_limited` is `true` for all
22 functions because Microsoft Learn excludes them from calculated columns and RLS rules in
DirectQuery mode; `APPROXIMATEDISTINCTCOUNT` still supports DirectQuery pushdown in measures.

### Blank policies

- `BlankWhenEmpty` — the result is BLANK when there are no rows or no qualifying value. Used
  where dax.guide and the observed engine agree, including the observed empty-set results of
  `SUMX`, `AVERAGEX`, `COUNTX` and `PRODUCTX`.
- `Unspecified` — dax.guide and Microsoft Learn disagree, or the sources are silent. This is
  deliberate: the catalog does not pick a winner for `AVERAGE`/`AVERAGEA`, `COUNTBLANK`,
  `MAX`/`MIN`/`MAXA`/`MINA` or `MAXX`/`MINX`.
- `ZeroWhenRowsButNoMatch` — documents the Microsoft Learn rule. No function uses it today
  because no engine observation supported it; it stays in the enum for a future evaluator that
  needs to model the conflicting rule explicitly.

### Static validation

`validate_expression` walks the AST and reports every argument-count or argument-kind error for a
catalogued function. Spans point at the offending argument for kind errors and at the whole call
for arity errors; unknown functions are ignored but their arguments are still visited.

A `ParamKind::Column` argument must be a bracketed `NameRef` (`Sales[Amount]` or `[Amount]`), so
`SUM ( 1 )` and `AVERAGE ( 1 + 1 )` are rejected. A `ParamKind::Table` argument is rejected only
when it is certainly scalar — a literal or an operator expression — so `COUNTROWS ( 1 )` fails
while `COUNTROWS ( DISTINCT ( Sales[Region] ) )`, table variables and `{ 1, 2 }` are accepted.
`COUNTROWS ()` is valid in the catalog (the Excel limitation is recorded on the function, not
enforced here). `Expression`, `Scalar`, `Logical` and `Any` are not statically decidable and are
recorded for documentation and future type checking, so the third `Variant` argument of
`MAXX`/`MINX` accepts any expression.

## Extending in later PRs

- `Query` currently holds only `evaluate`. `DEFINE`/`VAR` clauses (including query-level
  `VAR`) extend `Query` and `parse_query`; `Expr` and the operator grammar stay unchanged.
- The lexer already emits `=>` (`TokenKind::Arrow`) for user-defined functions.
- `Call`, `NameRef` and `NameStyle` keep spelling/quoting so catalog and type work can resolve
  names without re-lexing.

## Example

```sh
cargo run -p gridcraft-dax --example parse -- "1 + 2 * 3"
cargo run -p gridcraft-dax --example parse -- "EVALUATE SUM('Sales'[Amount])"
echo 'NOT TRUE || FALSE && TRUE' | cargo run -p gridcraft-dax --example parse
```
