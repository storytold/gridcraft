//! Localized dialects: pt-BR reference cases, round trips over every language/region pairing and
//! robustness against arbitrary input.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef};
use gridcraft_locale::{Dialect, LANGUAGES, REGIONS, language, region};

use crate::ast::*;
use crate::lexer::{Tok, tokenize_local};
use crate::{parse, parse_local, parse_local_with, print, print_local, print_local_with, print_r1c1_local};

fn dialect(lang: &str, reg: &str) -> Dialect<'static> {
    Dialect { names: language(lang).unwrap_or_else(|| panic!("language {lang}")), regional: region(reg).unwrap_or_else(|| panic!("region {reg}")) }
}

fn pt() -> Dialect<'static> {
    dialect("pt-BR", "pt-BR")
}

fn local(src: &str, d: &Dialect) -> Expr {
    parse_local(src, d).unwrap_or_else(|e| panic!("{src}: {e}"))
}

// ---------------------------------------------------------------- pt-BR reference cases

#[test]
fn pt_br_reference_cases() {
    let d = pt();
    assert_eq!(local("SE(A1>1,5;SOMA(B1:B3);0)", &d), parse("IF(A1>1.5,SUM(B1:B3),0)").unwrap());
    assert_eq!(local("=PROCV(A2;Tabela1[#Tudo];2;FALSO)", &d), parse("VLOOKUP(A2,Tabela1[#All],2,FALSE)").unwrap());
    assert_eq!(local("{1\\2;3\\4}", &d), parse("{1,2;3,4}").unwrap());
    assert_eq!(local("ÉCÉL.VAZIA(A1)", &d), parse("ISBLANK(A1)").unwrap());
    assert_eq!(local("écél.vazia(A1)", &d), parse("ISBLANK(A1)").unwrap());
    assert_eq!(local("ENDEREÇO(2;3;1;FALSO)", &d), parse("ADDRESS(2,3,1,FALSE)").unwrap());
    assert_eq!(local("SE(A1=#N/D;#NOME?;VERDADEIRO)", &d), parse("IF(A1=#N/A,#NAME?,TRUE)").unwrap());
    assert_eq!(local("1,5E+3+,5", &d), parse("1.5E+3+0.5").unwrap());
}

#[test]
fn spill_operator_in_local_dialects() {
    let pt = pt();
    let de = dialect("de-DE", "de-DE");
    let ja = dialect("ja-JP", "ja-JP");
    for (src, canonical, d) in [
        ("SOMA(A1#)", "SUM(A1#)", &pt),
        ("CONT.VALORES(Tabela1[Col];A1#)", "COUNTA(Tabela1[Col],A1#)", &pt),
        ("SE(A1#=#N/D;Planilha1!$B$2#;#NOME?)", "IF(A1#=#N/A,Planilha1!$B$2#,#NAME?)", &pt),
        ("SOMA(A1#;1,5)", "SUM(A1#,1.5)", &pt),
        ("{1\\2;3\\4}&A1#", "{1,2;3,4}&A1#", &pt),
        ("SUMME(A1#;B2#)", "SUM(A1#,B2#)", &de),
        ("WENN(A1#=#NV;'Tab 1'!A1#;#WERT!)", "IF(A1#=#N/A,'Tab 1'!A1#,#VALUE!)", &de),
        ("SUM(A1#,B2#)", "SUM(A1#,B2#)", &ja),
        ("@A1#", "@A1#", &pt),
    ] {
        let e = local(src, d);
        assert_eq!(e, parse(canonical).unwrap(), "{src}");
        assert_eq!(print(&e), canonical, "{src}");
        assert_eq!(local(&print_local(&e, d), d), e, "{src}");
    }
    assert_eq!(print_local(&parse("COUNTA(Tabela1[Col],A1#)").unwrap(), &pt), "CONT.VALORES(Tabela1[Col];A1#)");
    // The `#` right after a reference is the operator; anywhere else it starts an error literal.
    let toks = tokenize_local("A1#;#N/D", &pt).unwrap();
    let kinds: Vec<&Tok> = toks.iter().map(|t| &t.tok).collect();
    assert_eq!(kinds, [&Tok::Word("A1".into()), &Tok::Op("#"), &Tok::Comma, &Tok::Error(CellError::NA), &Tok::Eof]);
    let src = "SOMA(A1#)";
    let hash = tokenize_local(src, &pt).unwrap().into_iter().find(|t| t.tok == Tok::Op("#")).unwrap();
    assert_eq!(src.get(hash.start..hash.end), Some("#"));
    // Only a cell can spill; a localized error literal after a reference is not an operator use.
    assert!(parse_local("A1:B2#", &pt).is_err());
    assert!(parse_local("Nome#", &pt).is_err());
    assert!(parse_local("A1 #N/D", &pt).is_err());
    // Files spell the operator ANCHORARRAY whatever the dialect.
    assert_eq!(local("_xlfn.ANCHORARRAY(B2)", &pt), parse("B2#").unwrap());
    assert_eq!(print_local(&parse("#REF!").unwrap(), &pt), "#REF!");
    assert_eq!(parse("A1#").unwrap(), parse_local("A1#", &Dialect::INVARIANT).unwrap());
}

#[test]
fn pt_br_union_is_a_union() {
    let e = local("SOMA((A1;B1))", &pt());
    let Expr::Call(name, args) = &e else { panic!("{e:?}") };
    assert_eq!(name, "SUM");
    let [Expr::Paren(inner)] = args.as_slice() else { panic!("{args:?}") };
    assert!(matches!(**inner, Expr::Binary(BinOp::Union, _, _)), "{inner:?}");
    assert_eq!(e, parse("SUM((A1,B1))").unwrap());
}

#[test]
fn pt_br_rejects_english_names_like_excel() {
    let d = pt();
    let e = local("SUM(1;2)", &d);
    assert_eq!(e, Expr::Call("_XLUDF.SUM".into(), vec![Expr::Number(1.0), Expr::Number(2.0)]));
    assert_eq!(print_local(&e, &d), "SUM(1;2)");
    assert_eq!(print(&e), "_xludf.SUM(1,2)");
    // Canonical text re-reads to the same call.
    assert_eq!(parse("_xludf.SUM(1,2)").unwrap(), e);
    // Booleans and errors are the dialect's only.
    let err = parse_local("TRUE", &d).unwrap_err();
    assert!(err.msg.contains("VERDADEIRO"), "{err}");
    assert!(parse_local("#NAME?", &d).is_err());
    assert!(parse_local("1.5", &d).is_err());
    assert!(parse_local("SOMA(A1,B1)", &d).is_err());
}

#[test]
fn pt_br_prints_locally() {
    let d = pt();
    let e = parse("IF(A1>1.5,SUM(B1:B3),FALSE)+{1,2;3,4}&#N/A").unwrap();
    assert_eq!(print_local(&e, &d), "SE(A1>1,5;SOMA(B1:B3);FALSO)+{1\\2;3\\4}&#N/D");
    let e = parse("VLOOKUP(A2,Tabela1[[#Headers],[Col1]:[Col2]],2,TRUE)").unwrap();
    assert_eq!(print_local(&e, &d), "PROCV(A2;Tabela1[[#Cabeçalhos];[Col1]:[Col2]];2;VERDADEIRO)");
    assert_eq!(local(&print_local(&e, &d), &d), e);
}

#[test]
fn pt_br_r1c1_letters() {
    let e = parse("A1+$B$2+SUM(C:C)+SUM(3:4)").unwrap();
    let at = CellRef::new(1, 1);
    assert_eq!(print_r1c1_local(&e, at, &pt()), "L[-1]C[-1]+L2C2+SOMA(C[1])+SOMA(L[1]:L[2])");
    assert_eq!(crate::print_r1c1(&e, at), "R[-1]C[-1]+R2C2+SUM(C[1])+SUM(R[1]:R[2])");
}

#[test]
fn pt_br_structured_references() {
    let d = pt();
    for (src, canonical) in [
        ("Tabela1[#Tudo]", "Tabela1[#All]"),
        ("Tabela1[[#Cabeçalhos];[Vendas]]", "Tabela1[[#Headers],[Vendas]]"),
        ("Tabela1[[#Esta Linha];[Vendas]]", "Tabela1[[#This Row],[Vendas]]"),
        ("Tabela1[@Vendas]", "Tabela1[@Vendas]"),
        ("Tabela1[[#Totais];[A]:[C]]", "Tabela1[[#Totals],[A]:[C]]"),
        ("Tabela1[#dados]", "Tabela1[#Data]"),
    ] {
        assert_eq!(local(src, &d), parse(canonical).unwrap(), "{src}");
    }
    // English item names are plain column names in pt-BR.
    let Expr::Struct(s) = local("Tabela1[#All]", &d) else { panic!() };
    assert!(s.specifiers.is_empty());
}

// ---------------------------------------------------------------- other dialects

#[test]
fn de_de_arrays_and_numbers() {
    let d = dialect("de-DE", "de-DE");
    assert_eq!(local("{1.2;3.4}", &d), parse("{1,2;3,4}").unwrap());
    assert_eq!(local("{1,5.2;3.4}", &d), parse("{1.5,2;3,4}").unwrap());
    assert_eq!(local("{WAHR.FALSCH}", &d), parse("{TRUE,FALSE}").unwrap());
    assert_eq!(print_local(&parse("{1.5,2;3,4}").unwrap(), &d), "{1,5.2;3.4}");
    assert_eq!(local("SUMME(1,5;2)", &d), parse("SUM(1.5,2)").unwrap());
}

#[test]
fn ru_ru_array_rows_are_colons_not_row_ranges() {
    let d = dialect("ru-RU", "ru-RU");
    assert_eq!(local("{1;2:3;4}", &d), parse("{1,2;3,4}").unwrap());
    assert_eq!(local("{1:2:3}", &d), parse("{1;2;3}").unwrap());
    assert_eq!(print_local(&parse("{1,2;3,4}").unwrap(), &d), "{1;2:3;4}");
    // Outside braces `:` is still the range operator, whole rows included.
    assert_eq!(local("СУММ(1:3;A1:B2)", &d), parse("SUM(1:3,A1:B2)").unwrap());
}

#[test]
fn english_names_with_a_german_region() {
    let d = dialect("en-US", "de-DE");
    assert_eq!(local("SUM(1,5;2)", &d), parse("SUM(1.5,2)").unwrap());
    assert_eq!(local("{1.2;3.4}", &d), parse("{1,2;3,4}").unwrap());
    assert_eq!(print_local(&parse("IF(A1>1.5,TRUE,#N/A)").unwrap(), &d), "IF(A1>1,5;TRUE;#N/A)");
}

#[test]
fn portuguese_names_with_an_american_region() {
    let d = dialect("pt-BR", "en-US");
    assert_eq!(local("SE(A1>1.5,SOMA(B1:B3),FALSO)", &d), parse("IF(A1>1.5,SUM(B1:B3),FALSE)").unwrap());
    assert_eq!(local("{1,2;3,4}", &d), parse("{1,2;3,4}").unwrap());
}

#[test]
fn japanese_keeps_english_function_names() {
    let d = dialect("ja-JP", "ja-JP");
    assert_eq!(local("SUM(1,2)", &d), parse("SUM(1,2)").unwrap());
    assert_eq!(print_local(&parse("SUM(1,2)").unwrap(), &d), "SUM(1,2)");
}

#[test]
fn token_spans_are_local_byte_offsets() {
    let d = pt();
    let src = "SOMA(B2;A1;1,5)";
    let toks = tokenize_local(src, &d).unwrap();
    let a1 = toks.iter().find(|t| t.tok == Tok::Word("A1".into())).unwrap();
    assert_eq!(src.get(a1.start..a1.end), Some("A1"));
    let n = toks.iter().find(|t| matches!(t.tok, Tok::Number(_))).unwrap();
    assert_eq!(src.get(n.start..n.end), Some("1,5"));
    assert_eq!(n.tok, Tok::Number(1.5));
    let commas = toks.iter().filter(|t| t.tok == Tok::Comma).count();
    assert_eq!(commas, 2);
}

// ---------------------------------------------------------------- file-style names

#[test]
fn file_prefixes() {
    // Known built-ins lose the prefix, unknown ones keep it (lower case) and are not stripped.
    assert_eq!(parse("_xlfn.XLOOKUP(1,A:A,B:B)").unwrap(), parse("XLOOKUP(1,A:A,B:B)").unwrap());
    assert_eq!(parse("_xlfn._xlws.FILTER(A1:A3,B1:B3)").unwrap(), parse("FILTER(A1:A3,B1:B3)").unwrap());
    assert_eq!(parse("_xlws.SORT(A1:A3)").unwrap(), parse("SORT(A1:A3)").unwrap());
    assert_eq!(parse("_xlfn.NETWORKDAYS.INTL(A1,B1)").unwrap(), parse("NETWORKDAYS.INTL(A1,B1)").unwrap());
    for f in ["_xlfn.FORECAST.ETS(A1,B1:B9,C1:C9)", "_xlfn.SINGLE(A1)", "_xlfn._xlws.PY(A1)", "_xludf.MYFN(1)", "_xludf.SUM(1,2)"] {
        let e = parse(f).unwrap();
        assert_eq!(print(&e), f);
    }
    let Expr::Call(n, _) = parse("_xlfn.forecast.ets(1)").unwrap() else { panic!() };
    assert_eq!(n, "_XLFN.FORECAST.ETS");
    let Expr::Call(n, _) = parse("_xludf.sum(1)").unwrap() else { panic!() };
    assert_eq!(n, "_XLUDF.SUM");
}

#[test]
fn param_markers_are_stripped() {
    assert_eq!(parse("_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1)").unwrap(), parse("LAMBDA(x,x+1)").unwrap());
}

#[test]
fn parameter_calls_lose_the_marker_and_are_never_built_ins() {
    let imported = parse("_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1),_xlpm.f(1))").unwrap();
    assert_eq!(imported, parse("LET(f,LAMBDA(x,x+1),f(1))").unwrap());
    let Expr::Call(_, args) = &imported else { panic!("{imported:?}") };
    assert_eq!(args.last(), Some(&Expr::Call("F".into(), vec![Expr::Number(1.0)])));
    // Not resolved through a dialect either: `_xlpm.soma` stays a parameter call in pt-BR.
    assert_eq!(local("_xlpm.soma(1)", &pt()), Expr::Call("SOMA".into(), vec![Expr::Number(1.0)]));
}

#[test]
fn unresolved_names_are_upper_cased_in_ascii_only() {
    // The evaluator binds LET/LAMBDA names by ASCII case; `ação` must keep its non-ASCII letters.
    let e = parse("LET(ação,LAMBDA(x,x),ação(1))").unwrap();
    let Expr::Call(_, args) = &e else { panic!("{e:?}") };
    assert_eq!(args.get(2), Some(&Expr::Call("AçãO".into(), vec![Expr::Number(1.0)])));
    assert_eq!(args.first(), Some(&Expr::Name("ação".into())));
    assert_eq!(print(&e), "LET(ação,LAMBDA(x,x),AçãO(1))");
    // Same in a localized dialect, where only real function names are translated.
    let Expr::Call(n, _) = local("ação(1)", &pt()) else { panic!() };
    assert_eq!(n, "AçãO");
    let Expr::Call(n, _) = local("é.desconhecida(1)", &pt()) else { panic!() };
    assert_eq!(n, "é.DESCONHECIDA");
}

#[test]
fn canonical_booleans_are_rejected_in_a_dialect_without_them() {
    let d = pt();
    for src in ["TRUE", "false", "SE(TRUE;1;0)", "A1=FALSE"] {
        let err = parse_local(src, &d).unwrap_err();
        assert!(err.msg.contains("VERDADEIRO") || err.msg.contains("FALSO"), "{src}: {err}");
    }
    // The dialect's own literals and a dialect that shares the English ones are accepted.
    assert_eq!(local("SE(VERDADEIRO;1;FALSO)", &d), parse("IF(TRUE,1,FALSE)").unwrap());
    assert_eq!(local("TRUE", &dialect("ja-JP", "ja-JP")), Expr::Bool(true));
    // What parsed locally keeps its meaning through canonical text.
    let e = local("SE(A1=VERDADEIRO;1;0)", &d);
    assert_eq!(parse(&print(&e)).unwrap(), e);
}

// ---------------------------------------------------------------- names shadow local built-ins

#[test]
fn names_in_scope_shadow_local_function_names_and_booleans() {
    let es = dialect("es-ES", "es-ES");
    let known = |n: &str| ["SUMA", "FALSO"].iter().any(|k| k.eq_ignore_ascii_case(n));
    let read = |src: &str, known: &dyn Fn(&str) -> bool| print(&parse_local_with(src, &es, known).unwrap_or_else(|e| panic!("{src}: {e}")));
    assert_eq!(read("SUMA(2;3)+FALSO+SI(VERDADERO;4;5)", &known), "SUMA(2,3)+FALSO+IF(TRUE,4,5)");
    for (src, canonical) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        // A LET name is not in scope in its own value.
        ("LET(falso;FALSO;SI(falso;1;2))", "LET(falso,FALSE,IF(falso,1,2))"),
        ("LET(SUMA;LAMBDA(x;x+1);SUMA(2))", "LET(SUMA,LAMBDA(x,x+1),SUMA(2))"),
        ("LET(falso;7;valor;FALSO;valor)", "LET(falso,7,valor,FALSO,valor)"),
        ("LAMBDA(FALSO;FALSO)(7)", "LAMBDA(FALSO,FALSO)(7)"),
        ("LAMBDA(SUMA;SUMA(2))(LAMBDA(x;x+1))", "LAMBDA(SUMA,SUMA(2))(LAMBDA(x,x+1))"),
        // The scope ends with the call.
        ("LET(FALSO;7;FALSO)+FALSO+SUMA(1)", "LET(FALSO,7,FALSO)+FALSE+SUM(1)"),
        ("LET(FALSO;7;LAMBDA(x;FALSO+x)(1))", "LET(FALSO,7,LAMBDA(x,FALSO+x)(1))"),
    ] {
        assert_eq!(read(src, &crate::no_names), canonical, "{src}");
    }
    // A built-in whose local spelling is a name in scope is written so it still reads as the
    // built-in; a boolean becomes the TRUE()/FALSE() function, which has the same value.
    let e = parse("LET(SUMA,LAMBDA(x,x+1),SUM(SUMA(2),FALSE))").unwrap();
    let shown = print_local_with(&e, &es, &known);
    assert_eq!(shown, "LET(SUMA;LAMBDA(x;x+1);_xlfn.SUM(SUMA(2);_xlfn.FALSE()))");
    assert_eq!(read(&shown, &known), "LET(SUMA,LAMBDA(x,x+1),SUM(SUMA(2),FALSE()))");
    // Array constants hold no names.
    assert_eq!(read("{FALSO\\VERDADERO}", &known), "{FALSE,TRUE}");
}

#[test]
fn names_spelled_like_local_built_ins_round_trip_in_every_language() {
    for lang in LANGUAGES {
        let d = dialect(lang.tag, lang.tag);
        let sum = lang.local_function("SUM").unwrap_or("SUM");
        let falso = lang.bool_text(false);
        let known = |n: &str| n.eq_ignore_ascii_case(falso);
        let mut sources = vec![format!("LET({sum},LAMBDA(x,x+1),SUM({sum}(2),1))"), format!("LAMBDA({sum},{sum}(2)+SUM(3))(LAMBDA(x,x))")];
        if crate::parser::bindable_bool(falso) {
            sources.push(format!("IF({falso},LET({falso},2,{falso}+1),LAMBDA({falso},{falso})(3))"));
        }
        for src in sources {
            let e = parse(&src).unwrap_or_else(|err| panic!("{src}: {err}"));
            let text = print_local_with(&e, &d, &known);
            let back = parse_local_with(&text, &d, &known).unwrap_or_else(|err| panic!("{}: `{text}`: {err}", lang.tag));
            assert_eq!(back, e, "{}: `{text}`", lang.tag);
        }
    }
}

// ---------------------------------------------------------------- deep left-nested chains

fn on_small_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().unwrap()
}

#[test]
fn deep_left_nested_chains_do_not_overflow_a_small_stack() {
    on_small_stack(|| {
        let chains = [
            format!("1{}", "+1".repeat(5000)),
            format!("A1{}", "%".repeat(5000)),
            format!("LAMBDA(x,x){}", "(1)".repeat(3000)),
            format!("A1{}", ":A1".repeat(2000)),
            format!("1{}", "-A1%".repeat(1500)),
        ];
        let pt = pt();
        for src in chains {
            let e = parse(&src).unwrap();
            assert_eq!(print(&e), src);
            let local_text = print_local(&e, &pt);
            assert_eq!(parse_local(&local_text, &pt).unwrap(), e);
            assert!(!print_r1c1_local(&e, CellRef::new(0, 0), &pt).is_empty());
            let mut n = 0usize;
            e.walk(&mut |_| n += 1);
            assert!(n > 1500, "{n}");
            let mapped = e.clone().map(&mut |x| x);
            assert_eq!(mapped, e);
        }
    });
}

#[test]
fn walk_and_map_keep_their_order() {
    let e = parse("A1+B1%+SUM(C1,D1)").unwrap();
    let mut seen = Vec::new();
    e.walk(&mut |x| {
        if let Expr::Ref(r) = x {
            seen.push(crate::printer::reference_a1(r));
        }
    });
    assert_eq!(seen, ["A1", "B1", "C1", "D1"]);
    // Bottom-up: operands are visited before the nodes containing them.
    let mut order = Vec::new();
    let _ = e.map(&mut |x| {
        order.push(match &x {
            Expr::Ref(r) => crate::printer::reference_a1(r),
            Expr::Binary(..) => "bin".into(),
            Expr::Call(..) => "call".into(),
            Expr::Unary(..) => "pct".into(),
            _ => "other".into(),
        });
        x
    });
    assert_eq!(order, ["A1", "B1", "pct", "bin", "C1", "D1", "call", "bin"]);
}

// ---------------------------------------------------------------- generated round trips

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const SHEETS: [&str; 5] = ["Sheet 2", "Plan1", "Data", "Total.Sum", "it's"];
const NAMES: [&str; 5] = ["MyName", "Taxa_1", "Preço", "x", "Ünit.Cost"];
const TEXTS: [&str; 5] = ["abc", "a;b,c", "say \"hi\"", "1,5", "ação\\{}"];
const COLS: [&str; 5] = ["Col1", "Col 2", "Preço", "a;b", "x.y"];
const UDFS: [&str; 6] = ["MYFN", "_XLUDF.FOO", "_XLUDF.SUM", "_XLUDF.TEXT", "_XLFN.FORECAST.ETS", "_XLFN._XLWS.PY"];

fn gen_anchor(g: &mut Lcg) -> Anchor {
    Anchor { row: g.below(2000) as u32, col: g.below(100) as u32, row_abs: g.below(2) == 0, col_abs: g.below(2) == 0 }
}

fn gen_sheet(g: &mut Lcg) -> SheetSel {
    match g.below(6) {
        0 | 1 => SheetSel::Named((*g.pick(&SHEETS)).to_string()),
        2 => SheetSel::Span("Plan1".into(), "Plan3".into()),
        _ => SheetSel::Current,
    }
}

fn gen_ref(g: &mut Lcg) -> Reference {
    let kind = match g.below(5) {
        0 => RefKind::Range(gen_anchor(g), gen_anchor(g)),
        1 => RefKind::Rows(g.below(500) as u32, g.below(2) == 0, g.below(500) as u32 + 500, g.below(2) == 0),
        2 => RefKind::Cols(g.below(20) as u32, g.below(2) == 0, g.below(20) as u32 + 20, g.below(2) == 0),
        _ => RefKind::Cell(gen_anchor(g)),
    };
    let sheet = match (&kind, gen_sheet(g)) {
        // A 3-D span must stay a plain cell or range.
        (RefKind::Rows(..) | RefKind::Cols(..), SheetSel::Span(..)) => SheetSel::Current,
        (_, s) => s,
    };
    Reference { sheet, kind }
}

/// `A1#`: the spill operator applies to a single cell, possibly on another sheet.
fn gen_spill(g: &mut Lcg) -> Expr {
    let sheet = match gen_sheet(g) {
        SheetSel::Span(..) => SheetSel::Current,
        s => s,
    };
    Expr::Unary(UnOp::Spill, Box::new(Expr::Ref(Reference { sheet, kind: RefKind::Cell(gen_anchor(g)) })))
}

fn gen_number(g: &mut Lcg) -> f64 {
    match g.below(5) {
        0 => g.below(1000) as f64,
        1 => g.below(100000) as f64 / 100.0,
        2 => g.below(1000) as f64 / 8.0,
        3 => 1e20,
        _ => g.below(9) as f64 / 1000.0,
    }
}

fn gen_const(g: &mut Lcg) -> Expr {
    match g.below(6) {
        0 | 1 => Expr::Number(gen_number(g)),
        2 => Expr::Text(Arc::from(*g.pick(&TEXTS))),
        3 => Expr::Bool(g.below(2) == 0),
        4 => Expr::Error(*g.pick(&CellError::ALL)),
        _ => Expr::Number(gen_number(g)),
    }
}

fn gen_struct(g: &mut Lcg) -> StructRef {
    let table = (*g.pick(&["Tbl", "Tabela1", "Données"])).to_string();
    let col = |g: &mut Lcg| Some((*g.pick(&COLS)).to_string());
    let item = |g: &mut Lcg| *g.pick(&[StructItem::All, StructItem::Data, StructItem::Headers, StructItem::Totals]);
    match g.below(7) {
        0 => StructRef { table, specifiers: vec![], col_start: None, col_end: None },
        1 => StructRef { table, specifiers: vec![StructItem::ThisRow], col_start: None, col_end: None },
        2 => StructRef { table, specifiers: vec![item(g)], col_start: None, col_end: None },
        3 => StructRef { table, specifiers: vec![], col_start: col(g), col_end: None },
        4 => StructRef { table, specifiers: vec![StructItem::ThisRow], col_start: col(g), col_end: None },
        5 => StructRef { table, specifiers: vec![item(g)], col_start: col(g), col_end: col(g) },
        _ => StructRef { table, specifiers: vec![StructItem::Headers, StructItem::Data], col_start: col(g), col_end: None },
    }
}

fn gen_atom(g: &mut Lcg, depth: usize) -> Expr {
    match g.below(if depth == 0 { 6 } else { 9 }) {
        0 | 1 => gen_const(g),
        2 | 3 if g.below(5) == 0 => gen_spill(g),
        2 | 3 => Expr::Ref(gen_ref(g)),
        4 => Expr::Name((*g.pick(&NAMES)).to_string()),
        5 => Expr::Struct(gen_struct(g)),
        6 | 7 => gen_call(g, depth - 1),
        _ => Expr::Paren(Box::new(gen_expr(g, depth - 1))),
    }
}

fn gen_call(g: &mut Lcg, depth: usize) -> Expr {
    let name = if g.below(6) == 0 { (*g.pick(&UDFS)).to_string() } else { g.pick(crate::catalog::BUILTINS).0.to_string() };
    let n = g.below(4);
    let mut args: Vec<Expr> = (0..n).map(|_| if g.below(8) == 0 && n > 1 { Expr::Missing } else { gen_expr(g, depth) }).collect();
    if n == 1 && g.below(5) == 0 {
        args = vec![Expr::Array(gen_array(g))];
    }
    Expr::Call(name, args)
}

fn gen_array(g: &mut Lcg) -> Vec<Vec<Expr>> {
    let (rows, cols) = (1 + g.below(3), 1 + g.below(3));
    (0..rows).map(|_| (0..cols).map(|_| gen_const(g)).collect()).collect()
}

/// Expressions whose printed form parses back to the identical tree: binary operands are atoms
/// (parenthesised when compound) so no precedence is involved.
fn gen_expr(g: &mut Lcg, depth: usize) -> Expr {
    if depth == 0 {
        return gen_atom(g, 0);
    }
    match g.below(12) {
        0..=3 => {
            let op = *g.pick(&[
                BinOp::Add,
                BinOp::Sub,
                BinOp::Mul,
                BinOp::Div,
                BinOp::Pow,
                BinOp::Concat,
                BinOp::Eq,
                BinOp::Ne,
                BinOp::Lt,
                BinOp::Gt,
                BinOp::Le,
                BinOp::Ge,
            ]);
            Expr::Binary(op, Box::new(gen_atom(g, depth)), Box::new(gen_atom(g, depth)))
        }
        4 => {
            let plain = |g: &mut Lcg| Expr::Ref(Reference { sheet: SheetSel::Current, kind: RefKind::Cell(gen_anchor(g)) });
            Expr::Binary(BinOp::Intersect, Box::new(plain(g)), Box::new(plain(g)))
        }
        5 => {
            let plain = |g: &mut Lcg| Expr::Ref(gen_ref(g));
            let mut u = Expr::Binary(BinOp::Union, Box::new(plain(g)), Box::new(plain(g)));
            if g.below(2) == 0 {
                u = Expr::Binary(BinOp::Union, Box::new(u), Box::new(plain(g)));
            }
            Expr::Paren(Box::new(u))
        }
        6 => Expr::Unary(*g.pick(&[UnOp::Neg, UnOp::Plus, UnOp::At]), Box::new(gen_atom(g, 0))),
        7 => Expr::Unary(UnOp::Percent, Box::new(Expr::Ref(gen_ref(g)))),
        8 => Expr::Array(gen_array(g)),
        9 => Expr::Invoke(Box::new(Expr::Paren(Box::new(gen_call(g, depth - 1)))), (0..g.below(3)).map(|_| gen_atom(g, 0)).collect()),
        _ => gen_atom(g, depth),
    }
}

/// Parses with the dialect and returns the failing context for a readable assertion message.
fn assert_round_trip(e: &Expr, d: &Dialect) {
    let text = print_local(e, d);
    match parse_local(&text, d) {
        Ok(back) => assert_eq!(&back, e, "{}/{}: `{text}`", d.names.tag, d.regional.tag),
        Err(err) => panic!("{}/{}: `{text}` does not parse: {err}", d.names.tag, d.regional.tag),
    }
}

#[test]
fn generated_expressions_round_trip_in_every_dialect() {
    // Each language with its own region, then every pairing (mixed names/separators).
    let mut pairs: Vec<(&str, &str)> = LANGUAGES.iter().map(|l| (l.tag, l.tag)).collect();
    for l in LANGUAGES {
        for r in REGIONS {
            pairs.push((l.tag, r.tag));
        }
    }
    let mut g = Lcg(0x5EED);
    let exprs: Vec<Expr> = (0..300).map(|_| gen_expr(&mut g, 3)).collect();
    for (lang, reg) in pairs {
        let d = dialect(lang, reg);
        for e in &exprs {
            assert_round_trip(e, &d);
        }
    }
}

#[test]
fn generated_expressions_round_trip_canonically() {
    let mut g = Lcg(42);
    for _ in 0..2000 {
        let e = gen_expr(&mut g, 4);
        assert_round_trip(&e, &Dialect::INVARIANT);
        assert_eq!(parse(&print(&e)).unwrap(), e);
    }
}

#[test]
fn every_builtin_round_trips_in_every_language() {
    for l in LANGUAGES {
        let d = Dialect { names: l, regional: region(l.tag).unwrap_or_else(|| panic!("region {}", l.tag)) };
        for (name, _) in crate::catalog::BUILTINS {
            let e = Expr::Call((*name).to_string(), vec![Expr::Number(1.0), Expr::Number(2.5)]);
            assert_round_trip(&e, &d);
        }
    }
}

#[test]
fn printing_is_translation_free_for_user_text() {
    // Names, parameters, sheets, tables and quoted text keep their spelling in every dialect.
    let e = parse("LET(taxa,1.5,SUM(Plan1!A1,taxa,MyName,Tbl[Col 1],\"SUM(1,2)\"))").unwrap();
    for l in LANGUAGES {
        let d = Dialect { names: l, regional: region(l.tag).unwrap_or_else(|| panic!("region {}", l.tag)) };
        let t = print_local(&e, &d);
        for kept in ["taxa", "Plan1!A1", "MyName", "Tbl[Col 1]", "\"SUM(1,2)\""] {
            assert!(t.contains(kept), "{}: {t}", l.tag);
        }
        assert_round_trip(&e, &d);
    }
}

// ---------------------------------------------------------------- robustness

const ALPHABET: &[&str] = &[
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    ",",
    ";",
    ".",
    "\\",
    ":",
    "!",
    "'",
    "\"",
    "#",
    "@",
    "$",
    "%",
    "&",
    "<",
    ">",
    "=",
    "+",
    "-",
    "*",
    "/",
    "^",
    " ",
    "A",
    "B1",
    "1",
    "0",
    "5",
    "e",
    "E",
    "É",
    "ç",
    "ß",
    "あ",
    "SUM",
    "SOMA",
    "VERDADEIRO",
    "WAHR",
    "#N/D",
    "#NOME?",
    "#NAME?",
    "#REF!",
    "Tabela1[#Tudo]",
    "_xlfn.",
    "_xludf.",
    "_xlpm.",
    "R[1]C[-1]",
    "1,5",
    "1.5",
    "\u{0}",
    "\u{a0}",
    "\n",
];

fn mutate(g: &mut Lcg, s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    for _ in 0..1 + g.below(4) {
        let at = g.below(chars.len() + 1);
        match g.below(3) {
            0 => {
                chars.insert(at, g.pick(ALPHABET).chars().next().unwrap_or('('));
            }
            1 if !chars.is_empty() => {
                chars.remove(at.min(chars.len() - 1));
            }
            _ => {
                let piece: Vec<char> = g.pick(ALPHABET).chars().collect();
                for (k, c) in piece.into_iter().enumerate() {
                    chars.insert((at + k).min(chars.len()), c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn parsing_never_panics_in_any_dialect() {
    let mut dialects: Vec<Dialect<'static>> = LANGUAGES.iter().map(|l| dialect(l.tag, l.tag)).collect();
    dialects.push(dialect("pt-BR", "en-US"));
    dialects.push(dialect("en-US", "de-DE"));
    let mut g = Lcg(7);
    // Valid seeds to mutate: generated formulas printed in each dialect.
    let seeds: Vec<Expr> = (0..40).map(|_| gen_expr(&mut g, 3)).collect();
    for d in &dialects {
        for _ in 0..200 {
            let n = g.below(14);
            let random: String = (0..n).map(|_| *g.pick(ALPHABET)).collect();
            let _ = parse_local(&random, d);
            let _ = tokenize_local(&random, d);
        }
        for e in &seeds {
            let text = print_local(e, d);
            for _ in 0..12 {
                let m = mutate(&mut g, &text);
                if let Ok(parsed) = parse_local(&m, d) {
                    // Whatever parsed must print and re-read without panicking.
                    let _ = parse_local(&print_local(&parsed, d), d);
                }
                let _ = tokenize_local(&m, d);
            }
        }
    }
}
