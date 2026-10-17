//! Español (España): the formula words Spanish Excel shows (function names, booleans, errors,
//! structured-reference specifiers) and its formula separators.
//!
//! The function names are the ten the Spanish formula syntax already had (`VERDADERO()` and
//! `FALSO()` are the TRUE and FALSE functions). Functions not listed
//! keep their English name.
//!
//! Sources for the other words:
//! - bool: https://support.microsoft.com/es-es/excel/functions/true-function | https://raw.githubusercontent.com/PHPOffice/PhpSpreadsheet/master/src/PhpSpreadsheet/Calculation/locale/es/functions | https://raw.githubusercontent.com/handsontable/hyperformula/master/src/i18n/languages/esES.ts
//! - table_items: https://github.com/GrapeCity/spreadjs-resources (SpreadJS v18 ES resource: #Todo,#Datos,#Encabezados,#Totales,#Esta Fila) | https://support.microsoft.com/es-es/excel/using-structured-references-with-excel-tables (lists #Todas for #All; same #Datos,#Encabezados,#Totales,#Esta Fila)
//! - errors.numeric: https://support.microsoft.com/es-es/excel/functions/error-type-function | https://raw.githubusercontent.com/PHPOffice/PhpSpreadsheet/master/src/PhpSpreadsheet/Calculation/locale/es/config | https://github.com/LibreOffice/translations/blob/master/source/es/formula/messages.po
//! - errors.#N/A: https://support.microsoft.com/es-es/excel/functions/na-function (#N/D) | https://raw.githubusercontent.com/handsontable/hyperformula/master/src/i18n/languages/esES.ts (#N/D) | https://github.com/LibreOffice/translations/blob/master/source/es/formula/messages.po (#N/D) | https://es.stackoverflow.com/questions/538707 (cell value #N/D); error-type page table says #N/A
//! - errors.#SPILL!: https://raw.githubusercontent.com/handsontable/hyperformula/master/src/i18n/languages/esES.ts | https://github.com/LibreOffice/translations/blob/master/source/es/formula/messages.po (#¡DESBORDAMIENTO!) | https://support.microsoft.com/es-es/excel/how-to-correct-a-spill-error (drops the ¡)
//! - errors.#CALC!: https://github.com/ibarrasuarez22-ux/MC_regiduria (real Spanish Excel CSV exports contain #¡CALC!) | https://github.com/Carlos1200/gridcalc/blob/main/src/i18n/es.ts
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
        ("IF", "SI"),
        ("SUM", "SUMA"),
        ("SUMIF", "SUMAR.SI"),
        ("TODAY", "HOY"),
        ("TRUE", "VERDADERO"),
        ("VLOOKUP", "BUSCARV"),
    ],
};
