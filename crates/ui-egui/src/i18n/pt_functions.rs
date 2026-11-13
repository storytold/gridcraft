//! Brazilian Portuguese help for the worksheet functions. It covers the same 10 common functions
//! as the Spanish help (SUM, AVERAGE, IF…); the others keep their English help.

/// Function descriptions.
pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    ("Adds all the numbers in the arguments.", "Soma todos os números informados."),
    ("Adds the cells that meet a condition.", "Soma as células que atendem a uma condição."),
    ("Arithmetic mean of the numbers.", "Calcula a média aritmética dos números."),
    ("Counts the cells that meet a condition.", "Conta as células que atendem a uma condição."),
    ("Counts the numbers.", "Conta quantas células têm números."),
    (
        "Finds a value in the first column of a table and returns the value in the same row of another column.",
        "Procura um valor na primeira coluna de uma tabela e devolve o valor da mesma linha em outra coluna.",
    ),
    (
        "Returns one value when a condition is TRUE and another when it is FALSE.",
        "Devolve um valor quando a condição é VERDADEIRO e outro quando é FALSO.",
    ),
    ("Returns the current date.", "Devolve a data de hoje."),
    ("Returns the logical value FALSE.", "Devolve o valor lógico FALSO."),
    ("Returns the logical value TRUE.", "Devolve o valor lógico VERDADEIRO."),
];

/// Argument names without brackets or index (`number`, not `[number2]`).
pub(super) const ARGUMENTS: &[(&str, &str)] = &[
    ("col_index_num", "núm_coluna"),
    ("criteria", "critério"),
    ("logical_test", "teste_lógico"),
    ("lookup_value", "valor_procurado"),
    ("number", "número"),
    ("range", "intervalo"),
    ("range_lookup", "procurar_intervalo"),
    ("sum_range", "intervalo_soma"),
    ("table_array", "matriz_tabela"),
    ("value", "valor"),
    ("value_if_false", "valor_se_falso"),
    ("value_if_true", "valor_se_verdadeiro"),
];
