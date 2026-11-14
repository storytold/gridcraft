//! Italian help for the worksheet functions. It covers the 12 common functions the Italian
//! formula locale has aliases for (SUM, AVERAGE, IF…); the others keep their English help.

/// Function descriptions.
pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    ("Adds all the numbers in the arguments.", "Somma i numeri."),
    ("Adds the cells that meet a condition.", "Somma le celle che soddisfano un criterio."),
    ("Arithmetic mean of the numbers.", "Restituisce la media aritmetica dei numeri."),
    ("Counts the cells that meet a condition.", "Conta le celle che soddisfano un criterio."),
    ("Counts the numbers.", "Conta le celle che contengono numeri."),
    (
        "Finds a value in the first column of a table and returns the value in the same row of another column.",
        "Cerca un valore nella prima colonna di una tabella.",
    ),
    ("Returns one value when a condition is TRUE and another when it is FALSE.", "Restituisce un valore in base a una condizione."),
    ("Returns the current date and time.", "Restituisce la data e l'ora correnti."),
    ("Returns the current date.", "Restituisce la data odierna."),
    ("Returns the logical value FALSE.", "Restituisce il valore logico FALSO."),
    ("Returns the logical value TRUE.", "Restituisce il valore logico VERO."),
    (
        "Searches a row or column and returns the matching item, row or column from another array.",
        "Cerca un valore e restituisce un risultato corrispondente.",
    ),
];

/// Argument names without brackets or index (`number`, not `[number2]`).
pub(super) const ARGUMENTS: &[(&str, &str)] = &[
    ("col_index_num", "indice_colonna"),
    ("criteria", "criterio"),
    ("if_not_found", "se_non_trovato"),
    ("logical_test", "test_logico"),
    ("lookup_array", "in_cerca"),
    ("lookup_value", "cerca_valore"),
    ("match_mode", "modalità_corrispondenza"),
    ("number", "numero"),
    ("range", "intervallo"),
    ("range_lookup", "ordinato"),
    ("return_array", "ritorna"),
    ("search_mode", "modalità_ricerca"),
    ("sum_range", "intervallo_somma"),
    ("table_array", "matrice"),
    ("value", "valore"),
    ("value_if_false", "valore_se_falso"),
    ("value_if_true", "valore_se_vero"),
];
