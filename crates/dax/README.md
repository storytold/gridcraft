# gridcraft-dax

The DAX language for GridCraft: a tokenizer, a Pratt parser, a syntax tree and a canonical
printer. This crate is the foundation for later DAX PRs (aggregation, data types and
statements). It has no evaluator, no function catalog and no engine dependency.

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
