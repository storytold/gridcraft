//! Text functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category splits into three kinds of functions:
//!
//! - string construction (`COMBINEVALUES`, `CONCATENATE`, `CONCATENATEX`, `LEFT`, `MID`,
//!   `REPLACE`, `REPT`, `RIGHT`, `SUBSTITUTE`, `TRIM`), which return text and whose result
//!   length is measured in UTF-16 code units;
//! - classification and conversion (`EXACT`, `FIND`, `LEN`, `LOWER`, `SEARCH`, `UNICHAR`,
//!   `UNICODE`, `UPPER`), which are case- or accent-sensitive to different degrees;
//! - formatting and serialization (`FIXED`, `FORMAT`, `TOCSV`, `TOJSON`, `VALUE`), whose output
//!   or parsing depends on the model locale.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// Available to every host, with no documented DirectQuery restriction.
const STANDARD: FunctionFlags = FunctionFlags::new(false, false, false, true, false);

/// Functions that DirectQuery does not support in calculated columns.
const DQ_RESTRICTED: FunctionFlags = FunctionFlags::new(false, false, false, true, true);

/// `CONCATENATEX` is an iterator and is also restricted in DirectQuery calculated columns.
const ITERATOR: FunctionFlags = FunctionFlags::new(true, false, false, true, true);

/// Engine-only functions: no Excel version listed by dax.guide supports them.
const ENGINE_ONLY: FunctionFlags = FunctionFlags::new(false, false, false, false, true);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "COMBINEVALUES",
        FunctionCategory::Text,
        3,
        usize::MAX,
        &[required("Delimiter", ParamKind::Text), required("Expression1", ParamKind::Any), required("Expression2", ParamKind::Any)],
        &[variadic(&[ParamKind::Text, ParamKind::Any, ParamKind::Any])],
        ReturnType::String,
        STANDARD,
        "Concatenates two or more scalar values with a delimiter and returns one text value. The \
         function is designed for calculated columns that stand in for multi-column relationships \
         in DirectQuery models: the engine can translate the call into an optimized join \
         predicate, assuming without validating that different input combinations produce \
         different output strings. Choose a delimiter that cannot occur ambiguously in the \
         values, and remember that the delimiter itself is not escaped.",
        &["COMBINEVALUES ( \" | \", Customer[Customer Name], Customer[Customer Code] )", "COMBINEVALUES ( \"-\", \"Contoso\", \"2020\", \"Q1\" )"],
    ),
    spec(
        "CONCATENATE",
        FunctionCategory::Text,
        2,
        2,
        &[required("Text1", ParamKind::Text), required("Text2", ParamKind::Text)],
        &[overload(&[ParamKind::Text, ParamKind::Text])],
        ReturnType::String,
        STANDARD,
        "Joins two text values into one text value without inserting a separator. The `&` operator \
         performs the same concatenation and accepts any number of operands, so CONCATENATE is \
         only convenient for two. Non-text arguments are converted to text using the model's \
         locale rather than the caller's culture.",
        &["CONCATENATE ( \"Contoso\", \" 2020\" )", "CONCATENATE ( Customer[Customer Name], Customer[Customer Code] )"],
    ),
    spec(
        "CONCATENATEX",
        FunctionCategory::Text,
        2,
        5,
        &[
            required("Table", ParamKind::Table),
            required("Expression", ParamKind::Expression),
            optional("Delimiter", ParamKind::Text),
            optional("OrderBy_Expression", ParamKind::Expression),
            optional("Order", ParamKind::Logical),
        ],
        &[
            overload(&[ParamKind::Table, ParamKind::Expression]),
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Text]),
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Text, ParamKind::Expression]),
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Text, ParamKind::Expression, ParamKind::Logical]),
        ],
        ReturnType::String,
        ITERATOR,
        "Iterates a table, evaluates the expression for every row and concatenates the results \
         into a single text value using an optional delimiter. An order-by expression and an \
         optional order (0/FALSE/DESC or 1/TRUE/ASC, ascending by default) sort the values; \
         without them the row order is unspecified and does not reflect the source table's \
         physical order. Ordering follows the model collation, while the delimiter is inserted \
         verbatim.",
        &[
            "CONCATENATEX ( Sales, Sales[Product], \", \" )",
            "CONCATENATEX ( Sales, Sales[Product], \", \", Sales[Amount] )",
            "CONCATENATEX ( Sales, Sales[Product], \", \", Sales[Amount], TRUE )",
        ],
    ),
    spec(
        "EXACT",
        FunctionCategory::Text,
        2,
        2,
        &[required("Text1", ParamKind::Text), required("Text2", ParamKind::Text)],
        &[overload(&[ParamKind::Text, ParamKind::Text])],
        ReturnType::Boolean,
        STANDARD,
        "Compares two text values and returns TRUE when they are exactly the same, including \
         case. The ordinary `=` comparison is case-insensitive, so EXACT is the way to force a \
         case-sensitive and accent-sensitive test. An empty string and BLANK are considered the \
         same value.",
        &["EXACT ( \"SQLBI\", \"sqlbi\" )", "EXACT ( Sales[Code], \"A-100\" )"],
    ),
    spec(
        "FIND",
        FunctionCategory::Text,
        2,
        4,
        &[
            required("FindText", ParamKind::Text),
            required("WithinText", ParamKind::Text),
            optional("StartPosition", ParamKind::Number),
            optional("NotFoundValue", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Text, ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Number]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Number, ParamKind::Any]),
        ],
        ReturnType::Integer,
        STANDARD,
        "Returns the 1-based position of FindText inside WithinText, beginning the search at \
         StartPosition (default 1). The comparison is case-sensitive and accent-sensitive, and \
         wildcards are not interpreted; use SEARCH for case-insensitive or wildcard matching. If \
         the text is absent the function returns NotFoundValue when it is supplied, otherwise it \
         raises an error.",
        &["FIND ( \"Red\", \"Contoso 2G MP3 Player E200 Red\" )", "FIND ( \"Red\", \"Contoso 2G MP3 Player E200 Red\", 1, BLANK () )"],
    ),
    spec(
        "FIXED",
        FunctionCategory::Text,
        1,
        3,
        &[required("Number", ParamKind::Number), optional("Decimals", ParamKind::Number), optional("NoCommas", ParamKind::Boolean)],
        &[
            overload(&[ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Boolean]),
        ],
        ReturnType::String,
        DQ_RESTRICTED,
        "Rounds a number to the requested number of decimals and returns it as text, grouping the \
         integer part with commas unless NoCommas is TRUE. Decimals defaults to 2, and a negative \
         value rounds to the left of the decimal point. The decimal and grouping separators come \
         from the model locale, not from the caller's culture.",
        &["FIXED ( 384518.16, 2, FALSE )", "FIXED ( 4392768.29, 0, TRUE )", "FIXED ( 6741548.73, -2 )"],
    ),
    spec(
        "FORMAT",
        FunctionCategory::Text,
        2,
        3,
        &[required("Value", ParamKind::Any), required("Format", ParamKind::Text), optional("LocaleName", ParamKind::Text)],
        &[overload(&[ParamKind::Any, ParamKind::Text]), overload(&[ParamKind::Any, ParamKind::Text, ParamKind::Text])],
        ReturnType::String,
        DQ_RESTRICTED,
        "Formats a value as text using a Visual Basic (OLE Automation) format string or one of \
         the predefined numeric and date/time format names; unknown strings are treated as a \
         custom format or literal. The result is always text, and BLANK produces an empty string. \
         Month and weekday names and separators follow the model locale, which the optional \
         LocaleName overrides where the host supports that argument.",
        &["FORMAT ( 0.742, \"Percent\" )", "FORMAT ( DATE ( 2019, 3, 28 ), \"yyyy-mm-dd\" )", "FORMAT ( 1234.567, \"$#,0.00\", \"en-US\" )"],
    ),
    spec(
        "LEFT",
        FunctionCategory::Text,
        1,
        2,
        &[required("Text", ParamKind::Text), optional("NumberOfCharacters", ParamKind::Number)],
        &[overload(&[ParamKind::Text]), overload(&[ParamKind::Text, ParamKind::Number])],
        ReturnType::String,
        STANDARD,
        "Returns the first NumberOfCharacters characters of a text value; the count defaults to 1. \
         It is intended for fixed-position extraction. Characters are counted as UTF-16 code \
         units, and no case or locale conversion is applied to the result.",
        &["LEFT ( \"DAX is so cool!\", 3 )", "LEFT ( Customer[Customer Name] )"],
    ),
    spec(
        "LEN",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::Integer,
        STANDARD,
        "Returns the number of characters in a text value as an integer, counting spaces. The \
         length is measured in UTF-16 code units, so a character outside the Basic Multilingual \
         Plane counts as two.",
        &["LEN ( \"DAX is so cool!\" )", "LEN ( Customer[Customer Name] )"],
    ),
    spec(
        "LOWER",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::String,
        STANDARD,
        "Converts every letter in a text value to lowercase and returns the result. Case mapping \
         follows Unicode rules and is locale-sensitive for some scripts, such as the Turkish \
         dotted and dotless I, while digits and symbols are left unchanged.",
        &["LOWER ( \"DAX is SO cool!\" )", "LOWER ( Customer[CountryRegion] )"],
    ),
    spec(
        "MID",
        FunctionCategory::Text,
        3,
        3,
        &[required("Text", ParamKind::Text), required("StartPosition", ParamKind::Number), required("NumberOfCharacters", ParamKind::Number)],
        &[overload(&[ParamKind::Text, ParamKind::Number, ParamKind::Number])],
        ReturnType::String,
        STANDARD,
        "Returns NumberOfCharacters characters from a text value starting at the 1-based \
         StartPosition. If the requested length extends past the end of the text only the \
         remaining characters are returned. Positions and lengths are counted in UTF-16 code \
         units, and no case conversion is applied.",
        &["MID ( \"DAX is so cool!\", 11, 4 )", "MID ( \"123-45-6789\", 8, 3 )"],
    ),
    spec(
        "REPLACE",
        FunctionCategory::Text,
        4,
        4,
        &[
            required("OldText", ParamKind::Text),
            required("StartPosition", ParamKind::Number),
            required("NumberOfCharacters", ParamKind::Number),
            required("NewText", ParamKind::Text),
        ],
        &[overload(&[ParamKind::Text, ParamKind::Number, ParamKind::Number, ParamKind::Text])],
        ReturnType::String,
        DQ_RESTRICTED,
        "Replaces a fixed run of characters: NumberOfCharacters characters starting at the \
         1-based StartPosition are replaced by NewText, whose length need not match the removed \
         text. Because the position is explicit there is no searching, so case and locale do not \
         affect the match; positions are counted in UTF-16 code units.",
        &["REPLACE ( \"DAX is so cool!\", 11, 4, \"fantastic\" )", "REPLACE ( \"123-45-6789\", 1, 3, \"XXX\" )"],
    ),
    spec(
        "REPT",
        FunctionCategory::Text,
        2,
        2,
        &[required("Text", ParamKind::Text), required("NumberOfTimes", ParamKind::Number)],
        &[overload(&[ParamKind::Text, ParamKind::Number])],
        ReturnType::String,
        STANDARD,
        "Repeats a text value NumberOfTimes times and returns the concatenation. A count of zero \
         returns an empty string, a fractional count is rounded to the nearest integer, and the \
         result may not exceed 32,767 characters or the function raises an error. The text is \
         repeated verbatim, without case or locale conversion.",
        &["REPT ( \"?\", 3 )", "REPT ( \"0\", 5 )"],
    ),
    spec(
        "RIGHT",
        FunctionCategory::Text,
        1,
        2,
        &[required("Text", ParamKind::Text), optional("NumberOfCharacters", ParamKind::Number)],
        &[overload(&[ParamKind::Text]), overload(&[ParamKind::Text, ParamKind::Number])],
        ReturnType::String,
        STANDARD,
        "Returns the last NumberOfCharacters characters of a text value; the count defaults to 1. \
         It is intended for fixed-position extraction from the end of the text. Counting is by \
         UTF-16 code unit, and no case conversion is applied.",
        &["RIGHT ( \"DAX is so cool!\", 5 )", "RIGHT ( Customer[Customer Code], 4 )"],
    ),
    spec(
        "SEARCH",
        FunctionCategory::Text,
        2,
        4,
        &[
            required("FindText", ParamKind::Text),
            required("WithinText", ParamKind::Text),
            optional("StartPosition", ParamKind::Number),
            optional("NotFoundValue", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Text, ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Number]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Number, ParamKind::Any]),
        ],
        ReturnType::Integer,
        STANDARD,
        "Returns the 1-based position of FindText inside WithinText, starting at StartPosition \
         (default 1). Unlike FIND the comparison ignores case but remains accent-sensitive, and \
         the search text treats `?` and `*` as wildcards with `~` as the escape character. If the \
         text is absent the function returns NotFoundValue when supplied, otherwise it raises an \
         error.",
        &["SEARCH ( \"red\", \"Contoso 2G MP3 Player E200 Red\" )", "SEARCH ( \"player*blue\", \"Contoso 2G MP3 Player E200 Blue\", 1, BLANK () )"],
    ),
    spec(
        "SUBSTITUTE",
        FunctionCategory::Text,
        3,
        4,
        &[
            required("Text", ParamKind::Text),
            required("OldText", ParamKind::Text),
            required("NewText", ParamKind::Text),
            optional("InstanceNumber", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Text, ParamKind::Number]),
        ],
        ReturnType::String,
        STANDARD,
        "Replaces occurrences of OldText with NewText in a text value. With no InstanceNumber \
         every occurrence is replaced; otherwise only that occurrence is. Matching is \
         case-sensitive and accent-sensitive, so a difference in case leaves the text untouched, \
         and the search text is a literal substring rather than a pattern.",
        &[
            "SUBSTITUTE ( \"Contoso 2G MP3 Player E200 Red\", \"MP3 Player\", \"Zune-like player\" )",
            "SUBSTITUTE ( \"Contoso 2G MP3 Player E200 Red\", \"2\", \"?\", 1 )",
        ],
    ),
    spec(
        "TOCSV",
        FunctionCategory::Text,
        1,
        4,
        &[
            required("Table", ParamKind::Table),
            optional("MaxRows", ParamKind::Number),
            optional("Delimiter", ParamKind::Text),
            optional("IncludeHeaders", ParamKind::Boolean),
        ],
        &[
            overload(&[ParamKind::Table]),
            overload(&[ParamKind::Table, ParamKind::Number]),
            overload(&[ParamKind::Table, ParamKind::Number, ParamKind::Text]),
            overload(&[ParamKind::Table, ParamKind::Number, ParamKind::Text, ParamKind::Boolean]),
        ],
        ReturnType::String,
        ENGINE_ONLY,
        "Serializes a table's records as comma-separated text. MaxRows limits how many rows are \
         converted (default 10, negative for all), Delimiter is a non-empty constant field \
         separator (default a comma), and IncludeHeaders controls the header row (default TRUE). \
         Row order is unspecified and line endings are fixed; quotation marks are doubled, and a \
         value containing a quote or the delimiter is enclosed in quotes. Not available in Excel, \
         and not supported in DirectQuery calculated columns.",
        &["TOCSV ( Customer )", "TOCSV ( Customer, 3, \",\", FALSE )"],
    ),
    spec(
        "TOJSON",
        FunctionCategory::Text,
        1,
        2,
        &[required("Table", ParamKind::Table), optional("MaxRows", ParamKind::Number)],
        &[overload(&[ParamKind::Table]), overload(&[ParamKind::Table, ParamKind::Number])],
        ReturnType::String,
        ENGINE_ONLY,
        "Serializes a table's records as a JSON document with header, rowCount and data members. \
         MaxRows limits how many rows are converted (default 10, negative for all), while values \
         keep their native types inside the JSON text. Row order is unspecified. Not available in \
         Excel, and not supported in DirectQuery calculated columns.",
        &["TOJSON ( Customer )", "TOJSON ( Customer, 3 )"],
    ),
    spec(
        "TRIM",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::String,
        STANDARD,
        "Removes leading and trailing spaces and collapses runs of spaces between words to a \
         single space. Only the 7-bit ASCII space (character 32) is removed: the non-breaking \
         space (character 160), tabs and other whitespace survive, which matters for text copied \
         from web pages.",
        &["TRIM ( \"  DAX is          awesome !!!    \" )", "TRIM ( Customer[Customer Name] )"],
    ),
    spec(
        "UNICHAR",
        FunctionCategory::Text,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::String,
        STANDARD,
        "Returns the Unicode character for a numeric code point. The engine uses UTF-16, so a \
         supplementary character is represented by a surrogate pair. Code point 0 and values \
         outside the allowable Unicode range raise an error.",
        &["UNICHAR ( 9473 )", "UNICHAR ( 65 )"],
    ),
    spec(
        "UNICODE",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::Integer,
        STANDARD,
        "Returns the numeric Unicode value of the first character of a text value; an empty \
         string raises an error. The engine encodes text as UTF-16, so a character outside the \
         Basic Multilingual Plane is seen as two code units. Use UNICHAR for the inverse \
         conversion.",
        &["UNICODE ( \"A\" )", "UNICODE ( Customer[CountryRegion] )"],
    ),
    spec(
        "UPPER",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::String,
        STANDARD,
        "Converts every letter in a text value to uppercase and returns the result. Case mapping \
         follows Unicode rules and is locale-sensitive for some scripts, such as the Turkish \
         dotted and dotless I, while digits and symbols are left unchanged.",
        &["UPPER ( \"dax is so cool!\" )", "UPPER ( Customer[CountryRegion] )"],
    ),
    spec(
        "VALUE",
        FunctionCategory::Text,
        1,
        1,
        &[required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::Decimal,
        STANDARD,
        "Converts text that represents a number, date or time into its numeric value. Parsing \
         accepts the constant, number and date/time formats the engine recognizes and follows the \
         model locale for separators and date order; text that cannot be parsed raises an error. \
         Because the engine also coerces text implicitly, VALUE mainly makes the conversion \
         explicit, and it should be protected with IF when the column may contain invalid text.",
        &["VALUE ( \"123.45678\" )", "VALUE ( \"2020-10-23\" )", "VALUE ( Sales[TextAmount] )"],
    ),
];
