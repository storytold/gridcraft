//! Spanish help for the worksheet functions. It covers the 10 common functions Spanish had help
//! for (SUM, AVERAGE, IF…); the others keep their English help.

/// Function descriptions.
pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    ("Adds all the numbers in the arguments.", "Suma los números."),
    ("Adds the cells that meet a condition.", "Suma las celdas que cumplen un criterio."),
    ("Arithmetic mean of the numbers.", "Devuelve el promedio de los números."),
    ("Counts the cells that meet a condition.", "Cuenta las celdas que cumplen un criterio."),
    ("Counts the numbers.", "Cuenta las celdas que contienen números."),
    (
        "Finds a value in the first column of a table and returns the value in the same row of another column.",
        "Busca un valor en la primera columna de una tabla.",
    ),
    ("Returns one value when a condition is TRUE and another when it is FALSE.", "Devuelve un valor según una condición."),
    ("Returns the current date.", "Devuelve la fecha actual."),
    ("Returns the logical value FALSE.", "Devuelve el valor lógico FALSO."),
    ("Returns the logical value TRUE.", "Devuelve el valor lógico VERDADERO."),
];

/// Argument names without brackets or index (`number`, not `[number2]`).
pub(super) const ARGUMENTS: &[(&str, &str)] = &[
    ("col_index_num", "indicador_columnas"),
    ("criteria", "criterio"),
    ("logical_test", "prueba_lógica"),
    ("lookup_value", "valor_buscado"),
    ("number", "número"),
    ("range", "rango"),
    ("range_lookup", "ordenado"),
    ("sum_range", "rango_suma"),
    ("table_array", "matriz"),
    ("value", "valor"),
    ("value_if_false", "valor_si_falso"),
    ("value_if_true", "valor_si_verdadero"),
];
