//! Hungarian help for common worksheet functions.

pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    ("Adds all the numbers in the arguments.", "Összeadja az argumentumokban szereplő összes számot."),
    ("Adds the cells that meet a condition.", "Összeadja a feltételnek megfelelő cellák értékeit."),
    ("Arithmetic mean of the numbers.", "Kiszámítja a számok számtani átlagát."),
    ("Counts the cells that meet a condition.", "Megszámolja a feltételnek megfelelő cellákat."),
    ("Counts the numbers.", "Megszámolja a számokat."),
    (
        "Finds a value in the first column of a table and returns the value in the same row of another column.",
        "Megkeres egy értéket a táblázat első oszlopában, és visszaadja ugyanazon sor egy másik oszlopának értékét.",
    ),
    (
        "Returns one value when a condition is TRUE and another when it is FALSE.",
        "A feltételnek megfelelően az egyik vagy a másik értéket adja vissza.",
    ),
    ("Returns the current date.", "Visszaadja az aktuális dátumot."),
    ("Returns the logical value FALSE.", "A HAMIS logikai értéket adja vissza."),
    ("Returns the logical value TRUE.", "Az IGAZ logikai értéket adja vissza."),
];

pub(super) const ARGUMENTS: &[(&str, &str)] = &[];
