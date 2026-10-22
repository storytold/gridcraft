# gridcraft-dax

The DAX language for GridCraft: a tokenizer, a Pratt parser, a syntax tree, a canonical printer,
the aggregation function catalog with static call validation, the static type surface (types,
literals, inference and operator result tables) and the full DAX statement/query grammar. This
crate has no evaluator and no engine dependency.

```rust
use gridcraft_dax::{parse_expression, print_expression};

let expr = parse_expression("1 + 2 * 3")?;
assert_eq!(print_expression(&expr), "1 + 2 * 3");

let query = gridcraft_dax::parse_query(
    "DEFINE MEASURE Sales[Margin] = [Sales Amount] - [Total Cost] EVALUATE Sales ORDER BY [Margin] DESC",
)?;
assert!(gridcraft_dax::print_query(&query).starts_with("DEFINE\n    MEASURE Sales[Margin] ="));
# Ok::<(), gridcraft_dax::ParseError>(())
```

## Public API

| Item | Meaning |
| --- | --- |
| `parse_expression(&str) -> Result<Expr, ParseError>` | Parse one expression. One leading `=` (as typed in some editors) is accepted and ignored. |
| `print_expression(&Expr) -> String` | Canonical text. |
| `parse_query(&str) -> Result<Query, ParseError>` | Parse a query: optional `DEFINE`, then one or more `EVALUATE` statements. `EVALUATE <expr>` alone keeps working. |
| `print_query(&Query) -> String` | Canonical multi-line query text. |
| `tokenize(&str) -> Result<Vec<Token>, LexError>` | Tokens with byte spans, including `Eof`. |
| `ParseError { message, span }` / `LexError { message, span }` | `Display + std::error::Error`; the lexer error converts with `From`. |
| `ast::{Expr, ExprKind, Literal, DateTimeLiteral, Name, NameRef, NameStyle, Call, UnaryOp, BinaryOp, Span}` | Expression syntax tree. |
| `ast::{Query, Define, Definition, MeasureDef, ColumnDef, TableDef, VarDef, FunctionDef, MParameterDef}` | Query/`DEFINE` tree. |
| `ast::{Evaluate, OrderItem, StartValue, SortDirection}` | `EVALUATE`, `ORDER BY` and `START AT`. |
| `ast::{FunctionParam, ParameterType, ParameterSubtype, PassingMode}` | `FUNCTION` parameters and annotations. |
| `ast::{VisualShape, Axis, AxisKind, AxisGroup, AxisOrderItem}` | `WITH VISUAL SHAPE`, `AXIS`, `GROUP`, `TOTAL`. |
| `functions::{catalog, lookup}` | The 22 aggregation functions, looked up case-insensitively. |
| `functions::{FunctionSpec, FunctionCategory, ParamKind, ReturnType, BlankPolicy, FunctionFlags, Parameter, Overload}` | Catalog record types. |
| `validate_call(&Call) -> Vec<ValidationError>` | Arity/kind checks for one call; empty for valid or unknown functions. |
| `validate_expression(&Expr) -> Vec<ValidationError>` | Walks the tree and reports every invalid known-function call with its span. |
| `types::{DataType, PseudoType, InferredType, TypeMetadata, TypeError}` | The eight DAX types, the BLANK/Table pseudo-types, inference results and static type errors. |
| `types::{literal_type, infer_type, unary_type, binary_type, arithmetic_type, comparison_coercion, comparison_compatible, comparison_type}` | Literal classification, static inference and the documented operator/coercion tables. |
| `MAX_INPUT_LEN` (8192 bytes) / `MAX_DEPTH` (256) | Limits enforced with errors. |

### Syntax tree

```text
Expr { kind: ExprKind, span: Span }
ExprKind =
    Literal(Literal)                     Integer(i64) | Decimal(f64)
                                         | DateTime(DateTimeLiteral)
                                         | String(String) | Boolean(bool)
  | Name(NameRef)                        table: Option<Name>, name: Name
  | Call(Call)                           name: Name, args: Vec<Expr>
  | Unary { op, expr }                   op: UnaryOp (Plus | Neg | Not)
  | Binary { op, left, right }            op: BinaryOp
  | Table(Vec<Expr>)                     { value, ... }
  | Tuple(Vec<Expr>)                     (value, ...)
  | VarReturn { vars, body }             VAR a = 1 ... RETURN body

Query { define: Option<Define>, evaluates: Vec<Evaluate> }
Define { items: Vec<Definition> }
Definition =
    Measure(MeasureDef)                  name: NameRef, expr: Expr
  | Column(ColumnDef)                    name: NameRef, expr: Expr
  | Table(TableDef)                      name: Name, expr: Expr, visual_shape: Option<VisualShape>
  | Var(VarDef)                          name: Name, expr: Expr
  | Function(FunctionDef)                name: Name, params: Vec<FunctionParam>, body: Expr
  | MParameter(MParameterDef)            name: Name, expr: Expr

Evaluate { table: Expr, order_by: Vec<OrderItem>, start_at: Vec<StartValue> }
OrderItem { expr: Expr, direction: SortDirection }
StartValue = Literal(Literal) | Parameter(Name)          -- @name
FunctionParam { name, ty, subtype, passing, default }
VisualShape { axes: Vec<Axis>, densify: Option<String> }
Axis { kind: AxisKind (Rows | Columns), groups: Vec<AxisGroup>, order_by: Vec<AxisOrderItem> }
AxisGroup { columns: Vec<NameRef>, total: NameRef }
AxisOrderItem { column: NameRef, direction: SortDirection }
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
primary      := INTEGER | DECIMAL | STRING | TRUE | FALSE
              | dt STRING                            -- validated date/time literal
              | name
              | name "(" [ expression ("," expression)* ] ")"
              | table-qualified-name
              | "(" expression ")"
              | "(" expression "," expression ("," expression)* ")"
              | "{" [ expression ("," expression)* ] "}"
              | VAR var_name "=" expression { VAR var_name "=" expression } RETURN expression
name         := IDENT | "[" column "]" | "'" table "'"
qualified    := (IDENT | "'" table "'") "[" column "]"

query        := [ define ] evaluate { evaluate } [ ";" ]
define       := DEFINE definition { definition }
definition   := MEASURE table '[' name ']' "=" expression
              | COLUMN  table '[' name ']' "=" expression
              | TABLE   table_name "=" expression [ visual_shape ]
              | VAR     var_name "=" expression
              | FUNCTION function_name "=" "(" [ param { "," param } ] ")" "=>" expression
              | MPARAMETER mparameter_name "=" expression
param        := param_name [ ":" [ type ] [ subtype ] [ passing_mode ] ] [ "=" expression ]
evaluate     := EVALUATE expression [ order_by ] [ start_at ]
order_by     := ORDER BY order_item { "," order_item }
order_item   := expression [ ASC | DESC ]
start_at     := START AT start_value { "," start_value }
start_value  := literal | [ "+" | "-" ] number | "@" name
visual_shape := WITH VISUAL SHAPE axis { axis } [ DENSIFY STRING ]
axis         := AXIS ( ROWS | COLUMNS )
                    group { group }
                    ORDER BY column_ref [ ASC | DESC ] { "," column_ref [ ASC | DESC ] }
group        := GROUP column_ref { "," column_ref } TOTAL column_ref
column_ref   := "[" name "]" | table "[" name "]"
```

Statement keywords are matched case-insensitively. There are no statement terminators in the
documented grammar: `MEASURE`, `COLUMN`, `TABLE`, `VAR`, `FUNCTION`, `MPARAMETER` and the
`EVALUATE` token bound definition bodies and separate statements, so no newline is needed.
A single trailing `;` is accepted as a recovery extension. The `DEFINE` block is optional but
requires at least one entry and must be followed by at least one `EVALUATE`. Duplicate definition
names and forward/backward references are accepted; resolution is a semantic concern.

The shape above is illustrative: the Pratt parser applies the precedence and associativity
table below rather than a fixed grammar cascade.

`*` is not a wildcard token: it is only the multiplication operator.

### Statements

- `EVALUATE` takes any table expression. `ORDER BY` sorts the result and defaults to `ASC`;
  `START AT` requires `ORDER BY`, takes literals or `@parameter` references, and may not have
  more values than `ORDER BY` items.
- `DEFINE MEASURE`/`COLUMN` names are `<table>[<name>]`; `TABLE`, `FUNCTION` and `MPARAMETER`
  names are bare or single-quoted. `VAR` names are bare only. A query-scoped `VAR` has no
  `RETURN`; an expression-level `VAR ... RETURN ...` requires one. `RETURN` without a preceding
  `VAR` is an error.
- `FUNCTION` parameters may declare a type (`ANYVAL`, `SCALAR`, `TABLE`, `ANYREF`,
  `CALENDARREF`, `COLUMNREF`, `MEASUREREF`, `TABLEREF`), a scalar subtype (`BOOLEAN`,
  `DATETIME`, `DECIMAL`, `DOUBLE`, `INT64`, `NUMERIC`, `STRING`, `VARIANT`), a passing mode
  (`VAL`/`EXPR`) and a default expression. Only `SCALAR` (or an omitted type) accepts a subtype.
- `WITH VISUAL SHAPE` is valid only as the suffix of a `DEFINE TABLE`. It needs one or two axes
  (`AXIS ROWS`, `AXIS COLUMNS`, no duplicates), each with at least one `GROUP ... TOTAL` level
  and an `ORDER BY` list. `DENSIFY "column"` is optional and last, and takes a double-quoted
  string, not a bracketed reference. `AXIS ROWPAGES` is not part of the grammar.
- `MPARAMETER` is the Power BI dynamic M-parameter binding and is accepted as a `DEFINE` entry
  even though Microsoft's `DEFINE` grammar omits it.

### Case and escapes

- Keywords (`NOT`, `IN`, `EVALUATE`, `TRUE`, `FALSE`, and every statement keyword) are
  case-insensitive. `TRUE`/`FALSE` are literals unless followed by `(` (then they are calls to a
  function of that name). The printer writes statement keywords uppercase and keeps identifier
  spelling.
- Bare identifiers keep their spelling. Function names may contain `.` (`STDEV.P`).
- Single-quoted table identifiers double an embedded quote: `'It''s'`.
- Bracketed columns double an embedded `]`: `[A]]B]`.
- String literals double an embedded quote: `"a""b"`.
- Number literals are typed: a digit-only literal is an `Integer` (`42`, parsed as `i64`), while
  a decimal point or exponent is a `Decimal` (`3.14`, `.5`, `1.`, `1e3`, `1.5E-3`, parsed as
  `f64`). The sign is the unary operator, never part of the token. Values must be finite; an
  integer that does not fit `i64`, or a decimal that overflows `f64`, is a lexer error with a
  span. `Decimal` printing always keeps a decimal point or exponent (`1000.0`, `1e-6`) so the
  text re-lexes as a `Decimal`.
- Date/time literals are `dt"YYYY-MM-DD"`, `dt"YYYY-MM-DDThh:mm:ss"` and
  `dt"YYYY-MM-DD hh:mm:ss"`. The `dt` prefix is matched case-insensitively but must be adjacent
  to the quote; the printer emits lowercase `dt` and padded components. Unpadded month/day/time
  components are accepted (`dt"2015-1-9T2:3:0"`). Timezone offsets, fractional seconds,
  time-only forms, wrong-case `t`, and invalid calendar dates (including non-leap `1900-02-29`)
  are lexer errors.
- `BLANK` is an ordinary identifier. `BLANK()` is a zero-argument call; there is no BLANK literal.
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

## Data types and static inference

`types::DataType` is exactly the eight dax.guide types: `Binary`, `Boolean`, `Currency`,
`DateTime`, `Decimal`, `Integer`, `String`, `Variant`. `PseudoType` adds the two Microsoft DAX
types that are not in that list, `Blank` and `Table`; they are inference results, not
`DataType` variants. Each type (including the pseudo-types) has a `TypeMetadata` row with its
internal representation, literal syntax (or `None`), range, precision and notes. Literal
availability is `Boolean`, `DateTime`, `Decimal`, `Integer`, `String`; `Binary`, `Currency` and
`Variant` have no literal, and neither does BLANK (`BLANK()` is a function).

`literal_type(&Literal)` maps a literal to its `DataType`. `infer_type(&Expr) ->
Result<InferredType, TypeError>` infers literals, unary `+`/`-`/`NOT`, binary operators,
`TRUE()`/`FALSE()`/`BLANK()` calls, and table constructors. Name references and other function
calls return `InferredType::Unknown` because they need the model or a function catalog. `Unknown`
is not a DAX type; it is the absence of static information.

The arithmetic and comparison result tables are transcribed from the published DAX operator
reference for `Integer`, `Currency`, `Decimal` ("Real") and `DateTime`:
`arithmetic_type` implements `+`, `-`, `*`, `/` and `^` (a double). The quirks are preserved
rather than normalised: `Integer * DateTime -> Integer`, `Currency * Currency -> Decimal`,
`DateTime - DateTime -> DateTime`, and a `DateTime` on the left of `*` has no published row and is
a `TypeError`. `&` yields `String`; `&&`, `||`, comparisons and `IN` yield `Boolean`; unary `+`
and `-` preserve the numeric type. `Variant` propagates, and BLANK stays polymorphic (for example
`BLANK() + 5` is `Integer` while `BLANK() * 5` is `Blank`).

`comparison_coercion` models `=`, `<>` and ordering: BLANK is coerced to `0`, `""` or `FALSE` for
the other operand's type, and String/Boolean compared with a numeric type is a `TypeError` (the
engine errors; dax.guide also documents a conflicting type rank). `comparison_compatible` and
`comparison_type` also cover `==`, which is strict and does not coerce BLANK. Excel's embedded
engine rejects `==` altogether, but dax.guide documents it and this crate parses it.

This is static typing only. Text-to-number/date/Boolean parsing, BLANK substitution and truth
tables, `5/BLANK()` (Infinity), `0/BLANK()` (NaN), Currency rounding, runtime integer overflow,
IEEE equality and string collation belong to the evaluator. The module documentation lists the
documented conflicts (Currency/Integer endpoints, the DateTime minimum, mixed-type comparisons,
and `dt` literal engine support) instead of hiding them behind one total order.

## Limits, errors and printing

Input is capped at 8192 bytes and expression nesting at 256 levels; both produce an error, never a
stack overflow. The printer and `Expr::walk` use explicit stacks, and the parser's binary loop is
iterative for left-associated chains.

Numeric literal overflow (an integer outside `i64`, or a decimal outside finite `f64`) and invalid
date/time literals are lexer errors with the literal's span. Errors carry `message` and a byte
`span`; `Display` includes both. The canonical printer uses
spaces around binary operators, `, ` between arguments/items, and adds parentheses only where
precedence or associativity requires them. Re-parsing canonical text yields the same AST.

`print_query` prints `DEFINE` entries indented one level, each following `EVALUATE` on its own
line with indented `ORDER BY`/`START AT` clauses, and visual-shape axes, groups and `DENSIFY`
indented under their `TABLE`. The simple `EVALUATE <expression>` form stays on one line, so
`print_query(parse_query("EVALUATE 1 + 2"))` is still `EVALUATE 1 + 2`. An expression-level
`VAR ... RETURN ...` prints on one line and is parenthesised when it is an operand, because it
binds looser than every operator. All printing remains iterative.

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

- Statement semantics (name resolution, duplicate-name shadowing, `START AT` value types,
  function overloading and recursion rules, visual-shape evaluation) are not checked here; the
  parser accepts what the grammar accepts. Query-defined tables/columns and `MPARAMETER` are
  engine-specific, and the crate records them without validating a model.
- `Call`, `NameRef` and `NameStyle` keep spelling/quoting so catalog and type work can resolve
  names without re-lexing.
- `types::infer_type` deliberately knows only `TRUE()`, `FALSE()` and `BLANK()`, plus the
  operators and literals. The function PR supplies signatures for everything else;
  `InferredType::Unknown` marks those calls today, and a `VAR`/`RETURN` block is typed by its
  `RETURN` expression.

## Example

```sh
cargo run -p gridcraft-dax --example parse -- "1 + 2 * 3"
cargo run -p gridcraft-dax --example parse -- "EVALUATE SUM('Sales'[Amount])"
cargo run -p gridcraft-dax --example parse -- "DEFINE MEASURE Sales[M] = SUM(Sales[Amount]) EVALUATE Sales ORDER BY [M] DESC"
echo 'NOT TRUE || FALSE && TRUE' | cargo run -p gridcraft-dax --example parse
```
