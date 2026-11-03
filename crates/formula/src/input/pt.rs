//! Formula names and error spellings for PtBr, adapted from PR #2754.
use gridcraft_core::CellError;

pub(super) static DATA: super::LocaleData = super::LocaleData {
    functions: FUNCTIONS,
    errors: ERRORS,
    separators: super::COMMA_DECIMAL_SEPARATORS,
    fold_alias: str::to_lowercase,
    aliases: std::sync::OnceLock::new(),
};

const FUNCTIONS: &[(&str, &str)] = &[
    ("SUM", "SOMA"),
    ("AVERAGE", "MÉDIA"),
    ("IF", "SE"),
    ("COUNT", "CONT.NÚM"),
    ("COUNTA", "CONT.VALORES"),
    ("SUMIF", "SOMASE"),
    ("SUMIFS", "SOMASES"),
    ("COUNTIF", "CONT.SE"),
    ("COUNTIFS", "CONT.SES"),
    ("SUMPRODUCT", "SOMARPRODUTO"),
    ("VLOOKUP", "PROCV"),
    ("HLOOKUP", "PROCH"),
    ("XLOOKUP", "PROCX"),
    ("INDEX", "ÍNDICE"),
    ("MATCH", "CORRESP"),
    ("ROUND", "ARRED"),
    ("ROUNDUP", "ARREDONDAR.PARA.CIMA"),
    ("ROUNDDOWN", "ARREDONDAR.PARA.BAIXO"),
    ("TRUNC", "TRUNCAR"),
    ("MAX", "MÁXIMO"),
    ("MIN", "MÍNIMO"),
    ("AND", "E"),
    ("OR", "OU"),
    ("NOT", "NÃO"),
    ("IFERROR", "SEERRO"),
    ("CONCATENATE", "CONCATENAR"),
    ("TODAY", "HOJE"),
    ("NOW", "AGORA"),
    // Financial block, as in Spanish (#1986).
    ("NPV", "VPL"),
    ("IRR", "TIR"),
    ("PMT", "PGTO"),
    ("PV", "VP"),
    ("FV", "VF"),
    ("TRUE", "VERDADEIRO"),
    ("FALSE", "FALSO"),
];

const ERRORS: &[(CellError, &str)] = &[
    (CellError::Value, "#VALOR!"),
    (CellError::Name, "#NOME?"),
    (CellError::Num, "#NÚM!"),
    (CellError::NA, "#N/D"),
    (CellError::Spill, "#DESPEJAR!"),
];

#[cfg(test)]
mod tests {
    use crate::{FormulaLocale, parse_input, print, print_input};

    /// Checks each mapped name with decimal commas and argument separators.
    #[test]
    fn aliases_round_trip() {
        let locale = FormulaLocale::PtBr;
        for &(canonical, local) in super::FUNCTIONS {
            assert_eq!(locale.canonical_name(local), canonical, "{local}");
            let input = format!("{local}(1,25;2)");
            let expr = parse_input(&input, locale, |_| false).unwrap();
            assert_eq!(print(&expr), format!("{canonical}(1.25,2)"));
            assert_eq!(print_input(&expr, locale, |_| false).unwrap(), input);
        }
        for &(error, spelling) in super::ERRORS {
            let expr = parse_input(&spelling.to_lowercase(), locale, |_| false).unwrap();
            assert_eq!(print(&expr), error.as_str());
            assert_eq!(print_input(&expr, locale, |_| false).unwrap(), spelling);
        }

        let array = r"SOMA({1,5\2;3\4};0)";
        let expr = parse_input(array, locale, |_| false).unwrap();
        assert_eq!(print(&expr), "SUM({1.5,2;3,4},0)");
        assert_eq!(print_input(&expr, locale, |_| false).unwrap(), array);
    }
}
