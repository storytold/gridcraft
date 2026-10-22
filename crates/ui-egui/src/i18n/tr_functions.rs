//! Turkish help for common worksheet functions. Function identifiers and stored formulas stay
//! canonical English; descriptions and argument hints follow the interface language.

pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[
    ("Absolute value of a number.", "Bir sayının mutlak değerini döndürür."),
    ("Adds all the numbers in the arguments.", "Bağımsız değişkenlerdeki tüm sayıları toplar."),
    ("Adds the cells that meet a condition.", "Bir koşulu sağlayan hücreleri toplar."),
    ("Arithmetic mean of the numbers.", "Sayıların aritmetik ortalamasını döndürür."),
    ("Counts empty cells and empty text in a range.", "Bir aralıktaki boş hücreleri ve boş metinleri sayar."),
    ("Counts the cells that meet a condition.", "Bir koşulu sağlayan hücreleri sayar."),
    ("Counts the characters in a text.", "Bir metindeki karakterleri sayar."),
    ("Counts the non-empty values.", "Boş olmayan değerleri sayar."),
    ("Counts the numbers.", "Sayıları sayar."),
    (
        "Finds a value in the first column of a table and returns the value in the same row of another column.",
        "Tablonun ilk sütununda bir değer bulur ve başka bir sütunun aynı satırındaki değeri döndürür.",
    ),
    ("Largest number (0 when there are none).", "En büyük sayıyı döndürür (sayı yoksa 0)."),
    (
        "Removes leading and trailing spaces and collapses runs of spaces to one.",
        "Baştaki ve sondaki boşlukları kaldırır; ardışık boşlukları tek boşluğa indirir.",
    ),
    (
        "Returns a fallback when the value is an error, otherwise the value.",
        "Değer bir hataysa alternatif değeri, değilse değerin kendisini döndürür.",
    ),
    (
        "Returns one value when a condition is TRUE and another when it is FALSE.",
        "Koşul doğru olduğunda bir değer, yanlış olduğunda başka bir değer döndürür.",
    ),
    ("Returns the current date.", "Geçerli tarihi döndürür."),
    (
        "Returns the element, row or column of an array at the given position.",
        "Bir dizinin belirtilen konumundaki öğeyi, satırı veya sütunu döndürür.",
    ),
    ("Returns the first characters of a text.", "Bir metnin başındaki karakterleri döndürür."),
    ("Returns the last characters of a text.", "Bir metnin sonundaki karakterleri döndürür."),
    ("Returns the logical value FALSE.", "Yanlış mantıksal değerini döndürür."),
    ("Returns the logical value TRUE.", "Doğru mantıksal değerini döndürür."),
    ("Reverses a logical value.", "Bir mantıksal değerin tersini döndürür."),
    (
        "Rounds a number to a given number of digits, halves away from zero.",
        "Bir sayıyı belirtilen basamak sayısına yuvarlar; yarım değerleri sıfırdan uzağa yuvarlar.",
    ),
    ("Smallest number (0 when there are none).", "En küçük sayıyı döndürür (sayı yoksa 0)."),
    ("TRUE when at least one argument is TRUE.", "En az bir bağımsız değişken doğruysa doğru döndürür."),
    ("TRUE when every argument is TRUE.", "Tüm bağımsız değişkenler doğruysa doğru döndürür."),
];

pub(super) const ARGUMENTS: &[(&str, &str)] = &[
    ("area_num", "alan_sayısı"),
    ("array", "dizi"),
    ("col_index_num", "sütun_indis_sayısı"),
    ("column_num", "sütun_sayısı"),
    ("criteria", "ölçüt"),
    ("logical", "mantıksal"),
    ("logical_test", "mantıksal_sınama"),
    ("lookup_value", "aranan_değer"),
    ("num_chars", "karakter_sayısı"),
    ("num_digits", "basamak_sayısı"),
    ("number", "sayı"),
    ("range", "aralık"),
    ("range_lookup", "aralık_bak"),
    ("row_num", "satır_sayısı"),
    ("sum_range", "toplam_aralığı"),
    ("table_array", "tablo_dizisi"),
    ("text", "metin"),
    ("value", "değer"),
    ("value_if_error", "hata_ise_değer"),
    ("value_if_false", "yanlış_ise_değer"),
    ("value_if_true", "doğru_ise_değer"),
];
