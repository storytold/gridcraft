//! Español (España): the formula words Spanish Excel shows (function names, booleans, errors,
//! structured-reference specifiers) and its formula separators.
//!
//! The function names are the ten the Spanish formula syntax already had (`VERDADERO()` and
//! `FALSO()` are the TRUE and FALSE functions) plus the financial block added upstream in #2000
//! (`VNA`, `TIR`, `PAGO`, `VA`, `VF`). Functions not listed keep their English name.
//!
//! Sources for the other words (Microsoft's pages, permissively licensed references and files
//! exported by Spanish Excel; no copyleft project is used):
//! - bool: https://support.microsoft.com/es-es/excel/functions/true-function | https://raw.githubusercontent.com/PHPOffice/PhpSpreadsheet/master/src/PhpSpreadsheet/Calculation/locale/es/functions (MIT)
//! - table_items: https://github.com/GrapeCity/spreadjs-resources (MIT; SpreadJS v18 ES resource: #Todo,#Datos,#Encabezados,#Totales,#Esta Fila) | https://support.microsoft.com/es-es/excel/using-structured-references-with-excel-tables (lists #Todas for #All; same #Datos,#Encabezados,#Totales,#Esta Fila)
//! - errors.numeric: https://support.microsoft.com/es-es/excel/functions/error-type-function | https://raw.githubusercontent.com/PHPOffice/PhpSpreadsheet/master/src/PhpSpreadsheet/Calculation/locale/es/config (MIT)
//! - errors.#N/A: https://support.microsoft.com/es-es/excel/functions/na-function (#N/D) | https://es.stackoverflow.com/questions/538707 (cell value #N/D); error-type page table says #N/A
//! - errors.#SPILL!: unconfirmed. https://support.microsoft.com/es-es/excel/how-to-correct-a-spill-error writes #DESBORDAMIENTO!; the ¡ follows every other Spanish error (#¡VALOR!, #¡REF!) until Spanish Excel is observed
//! - errors.#CALC!: https://github.com/ibarrasuarez22-ux/MC_regiduria (real Spanish Excel CSV exports contain #¡CALC!)
//! - errors.#GETTING_DATA: https://support.microsoft.com/es-es/excel/functions/error-type-function

use super::Dialect;
use gridcraft_core::CellError;

pub(super) const DIALECT: Dialect = Dialect {
    list: ';',
    decimal: ',',
    array_column: '\\',
    array_row: ';',
    bool_true: "VERDADERO",
    bool_false: "FALSO",
    errors: &[
        (CellError::Null, "#¡NULO!"),
        (CellError::Div0, "#¡DIV/0!"),
        (CellError::Value, "#¡VALOR!"),
        (CellError::Ref, "#¡REF!"),
        (CellError::Name, "#¿NOMBRE?"),
        (CellError::Num, "#¡NUM!"),
        (CellError::NA, "#N/D"),
        (CellError::GettingData, "#OBTENIENDO_DATOS"),
        (CellError::Spill, "#¡DESBORDAMIENTO!"),
        (CellError::Calc, "#¡CALC!"),
    ],
    table_items: ["#Todo", "#Datos", "#Encabezados", "#Totales", "#Esta fila"],
    functions: &[
        ("AVERAGE", "PROMEDIO"),
        ("COUNT", "CONTAR"),
        ("COUNTIF", "CONTAR.SI"),
        ("FALSE", "FALSO"),
        ("FV", "VF"),
        ("IF", "SI"),
        ("IRR", "TIR"),
        ("NPV", "VNA"),
        ("PMT", "PAGO"),
        ("PV", "VA"),
        ("SUM", "SUMA"),
        ("SUMIF", "SUMAR.SI"),
        ("TODAY", "HOY"),
        ("TRUE", "VERDADERO"),
        ("VLOOKUP", "BUSCARV"),
    ],
};
