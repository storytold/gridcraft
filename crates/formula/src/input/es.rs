//! Spanish formula-entry data, moved from the original shared input module.

pub(super) static DATA: super::LocaleData = super::LocaleData {
    functions: FUNCTIONS,
    errors: &[],
    separators: super::COMMA_DECIMAL_SEPARATORS,
    fold_alias: str::to_lowercase,
    aliases: std::sync::OnceLock::new(),
};

const FUNCTIONS: &[(&str, &str)] = &[
    ("SUM", "SUMA"),
    ("AVERAGE", "PROMEDIO"),
    ("IF", "SI"),
    ("COUNT", "CONTAR"),
    ("SUMIF", "SUMAR.SI"),
    ("COUNTIF", "CONTAR.SI"),
    ("VLOOKUP", "BUSCARV"),
    ("TODAY", "HOY"),
    // Financial block (#1986): the loan-schedule and investment staples.
    ("NPV", "VNA"),
    ("IRR", "TIR"),
    ("PMT", "PAGO"),
    ("PV", "VA"),
    ("FV", "VF"),
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];
