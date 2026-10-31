//! Math and trigonometry functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, standard};

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "ABS",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Numeric,
        standard(false),
        "Returns the absolute value of a number, preserving its numeric type (Currency, Decimal \
         or Integer) and removing only the sign. Infinite values are supported; no rounding is \
         performed.",
        &["ABS ( -1 )", "ABS ( [Amount] )"],
    ),
    spec(
        "ACOS",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the arccosine of a number in radians, in the range 0 to pi. The argument is the \
         cosine of the angle and must be between -1 and 1; multiply the result by 180/PI() or \
         use DEGREES to obtain degrees. Results are approximate Decimal values.",
        &["ACOS ( 0.5 )"],
    ),
    spec(
        "ACOSH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the inverse hyperbolic cosine, in radians, of a number greater than or equal to \
         1. ACOSH(COSH(x)) equals x when x is in range; the result is an approximate Decimal \
         value.",
        &["ACOSH ( 2 )"],
    ),
    spec(
        "ACOT",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the principal value of the arccotangent of a real number, in radians in the \
         range 0 to pi. The result is an approximate Decimal value.",
        &["ACOT ( 1 )"],
    ),
    spec(
        "ACOTH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the inverse hyperbolic cotangent of a number whose absolute value is greater \
         than 1. The result is an approximate Decimal value.",
        &["ACOTH ( 2 )"],
    ),
    spec(
        "ASIN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the arcsine of a number in radians, in the range -pi/2 to pi/2. The argument is \
         the sine of the angle and must be between -1 and 1; multiply by 180/PI() or use DEGREES \
         for degrees. Results are approximate Decimal values.",
        &["ASIN ( 0.5 )"],
    ),
    spec(
        "ASINH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the inverse hyperbolic sine of any real number, in radians. ASINH(SINH(x)) \
         equals x; the result is an approximate Decimal value.",
        &["ASINH ( 1 )"],
    ),
    spec(
        "ATAN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the arctangent of a number in radians, in the range -pi/2 to pi/2. Multiply by \
         180/PI() or use DEGREES for degrees; the result is an approximate Decimal value.",
        &["ATAN ( 1 )"],
    ),
    spec(
        "ATANH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the inverse hyperbolic tangent of a number strictly between -1 and 1. A value \
         of -1, 1 or outside that range produces an error; the result is an approximate Decimal \
         value.",
        &["ATANH ( 0.5 )"],
    ),
    spec(
        "CEILING",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("Significance", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Numeric,
        standard(false),
        "Rounds a number up to the nearest multiple of significance. Unlike ISO.CEILING, a \
         negative significance makes CEILING round negative numbers toward negative infinity. \
         The result usually follows the type of Significance, but a Currency Number stays \
         Currency, a Boolean Significance yields Integer and a non-numeric Significance yields \
         Decimal.",
        &["CEILING ( 10.2, 1 )", "CEILING ( 10.7, 0.5 )"],
    ),
    spec(
        "CONVERT",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Expression", ParamKind::Any), required("DataType", ParamKind::Any)],
        &[overload(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        standard(false),
        "Converts an expression to the requested data type. The type is an unquoted keyword: \
         BOOLEAN/LOGICAL, CURRENCY/DECIMAL, DATETIME, DOUBLE, INTEGER/INT64 or STRING/TEXT. \
         Numeric conversions follow the target type's precision, so CURRENCY/DECIMAL rounds to \
         four decimal places while DOUBLE keeps approximately 15 significant digits.",
        &["CONVERT ( \"123\", INTEGER )", "CONVERT ( 12345, DATETIME )"],
    ),
    spec(
        "COS",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the cosine of an angle given in radians. Convert degrees with RADIANS or by \
         multiplying by PI()/180; the result is an approximate Decimal value.",
        &["COS ( PI () / 3 )"],
    ),
    spec(
        "COSH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the hyperbolic cosine of a real number. The result is an approximate Decimal \
         value and grows quickly for large arguments.",
        &["COSH ( 1 )"],
    ),
    spec(
        "COT",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the cotangent of an angle given in radians, computed as 1/TAN(angle). It is \
         undefined at multiples of pi; the result is an approximate Decimal value.",
        &["COT ( PI () / 4 )"],
    ),
    spec(
        "COTH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the hyperbolic cotangent of a hyperbolic angle given in radians, computed as \
         1/TANH(angle). The result is an approximate Decimal value and is undefined at zero.",
        &["COTH ( 1 )"],
    ),
    spec(
        "CURRENCY",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Value", ParamKind::Scalar)],
        &[overload(&[ParamKind::Scalar])],
        ReturnType::Currency,
        standard(false),
        "Converts a scalar expression to the Currency type, which stores four decimal places. \
         The fifth significant decimal is rounded up when it is 5 or larger; TRUE becomes 1 and \
         FALSE becomes 0, and numeric text is parsed. Currency values are fixed-point, so this \
         conversion is not a general-purpose numeric cast.",
        &["CURRENCY ( 10.5 )", "CURRENCY ( \"3.66666\" )"],
    ),
    spec(
        "DEGREES",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Converts an angle from radians to degrees by multiplying by 180/pi. The result is an \
         approximate Decimal value; RADIANS performs the inverse conversion.",
        &["DEGREES ( PI () )"],
    ),
    spec(
        "DIVIDE",
        FunctionCategory::MathAndTrig,
        2,
        3,
        &[required("Numerator", ParamKind::Number), required("Denominator", ParamKind::Number), optional("AlternateResult", ParamKind::Any)],
        &[overload(&[ParamKind::Number, ParamKind::Number]), overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Any])],
        ReturnType::Decimal,
        standard(false),
        "Performs safe division and returns an alternate result when the denominator is zero, \
         avoiding the error or infinity produced by the / operator. The alternate result must be \
         a constant and defaults to BLANK; the quotient is an approximate Decimal value. DIVIDE \
         runs in the formula engine and is slower than native division when the denominator is \
         known to be non-zero.",
        &["DIVIDE ( 10, 2 )", "DIVIDE ( 10, 0, 0 )"],
    ),
    spec(
        "EVEN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Rounds a number away from zero to the nearest even integer. An even integer is returned \
         unchanged, and non-numeric input raises an error. The result is an exact Integer.",
        &["EVEN ( 3.2 )", "EVEN ( -3.2 )"],
    ),
    spec(
        "EXP",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns e raised to the given power, the inverse of LN. The result is an approximate \
         Decimal value with about 15 significant digits; for other bases use POWER or ^.",
        &["EXP ( 1 )"],
    ),
    spec(
        "FACT",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the factorial of a non-negative number in Decimal form. A non-integer argument \
         is truncated, and values above 170 overflow; the result is exact only through 21!. \
         Calculated columns and RLS rules cannot use FACT in DirectQuery mode.",
        &["FACT ( 5 )", "FACT ( 21 )"],
    ),
    spec(
        "FLOOR",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("Significance", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Rounds a number down, toward zero, to the nearest multiple of significance. Number and \
         Significance must have the same sign, and the function always returns a Decimal \
         regardless of the argument types.",
        &["FLOOR ( 2.7, 1 )", "FLOOR ( -2.7, 1 )"],
    ),
    spec(
        "GCD",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number1", ParamKind::Number), required("Number2", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Returns the greatest common divisor of two integers. Non-integer arguments are \
         truncated before the calculation; the result is an exact Integer.",
        &["GCD ( 24, 36 )"],
    ),
    spec(
        "INT",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Any,
        standard(false),
        "Rounds a number down toward negative infinity to an integer, so INT(-2.1) is -3. The \
         result is Integer for Integer or Decimal input and Currency for Currency input; unlike \
         TRUNC, the result type follows the argument and negative values move away from zero.",
        &["INT ( -2.9 )", "INT ( 2.9 )"],
    ),
    spec(
        "ISO.CEILING",
        FunctionCategory::MathAndTrig,
        1,
        2,
        &[required("Number", ParamKind::Number), optional("Significance", ParamKind::Number)],
        &[overload(&[ParamKind::Number]), overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Numeric,
        standard(false),
        "Rounds a number up to the nearest multiple of significance using ISO rules. When \
         Significance is omitted it defaults to 1; negative numbers and negative significance \
         can round differently from CEILING. The result keeps the numeric type of Number, with \
         Decimal precision when required by the multiple.",
        &["ISO.CEILING ( 10.2 )", "ISO.CEILING ( 10.7, 0.5 )"],
    ),
    spec(
        "LCM",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number1", ParamKind::Number), required("Number2", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Returns the least common multiple of two integers, the smallest positive integer that \
         both arguments divide without a remainder. Non-integer arguments are truncated; the \
         result is an exact Integer.",
        &["LCM ( 4, 6 )"],
    ),
    spec(
        "LN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the natural logarithm (base e) of a positive number. Zero and negative numbers \
         produce an error; the result is an approximate Decimal value and is the inverse of EXP.",
        &["LN ( 10 )"],
    ),
    spec(
        "LOG",
        FunctionCategory::MathAndTrig,
        1,
        2,
        &[required("Number", ParamKind::Number), optional("Base", ParamKind::Number)],
        &[overload(&[ParamKind::Number]), overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the logarithm of a positive number in the given base; omitting the base uses \
         10. The result is an approximate Decimal value, and LOG10 is the fixed base-10 \
         shortcut.",
        &["LOG ( 100 )", "LOG ( 8, 2 )"],
    ),
    spec(
        "LOG10",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the base-10 logarithm of a positive number. The result is an approximate \
         Decimal value; it is equivalent to LOG(number, 10).",
        &["LOG10 ( 1000 )"],
    ),
    spec(
        "MOD",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("Divisor", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Any,
        standard(false),
        "Returns the remainder after dividing Number by Divisor. The result follows the operand \
         type (Integer, Currency or Decimal) and its sign follows the divisor; a zero divisor \
         produces an error. QUOTIENT returns the corresponding integer portion.",
        &["MOD ( 7, 3 )", "MOD ( 8, 3 )"],
    ),
    spec(
        "MROUND",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("Multiple", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Rounds a number to the nearest multiple, rounding away from zero when the remainder is \
         at least half the multiple. Number and Multiple must have the same sign; for decimal \
         multiples the midpoint direction is undefined, so MROUND(6.05, 0.1) can differ from \
         MROUND(7.05, 0.1). The result is Decimal.",
        &["MROUND ( 7.3, 0.5 )"],
    ),
    spec(
        "ODD",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Rounds a number away from zero to the nearest odd integer. An odd integer is returned \
         unchanged, and non-numeric input raises an error. The result is an exact Integer.",
        &["ODD ( 2.1 )", "ODD ( -2.1 )"],
    ),
    spec(
        "PI",
        FunctionCategory::MathAndTrig,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Decimal,
        standard(false),
        "Returns the constant pi, 3.14159265358979, accurate to 15 digits as an approximate \
         Decimal value. It takes no arguments.",
        &["PI ()"],
    ),
    spec(
        "POWER",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("Power", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns Number raised to Power. It is equivalent to the ^ operator and produces an \
         approximate Decimal value; fractional powers of negative bases produce an error.",
        &["POWER ( 2, 10 )", "POWER ( 9, 0.5 )"],
    ),
    spec(
        "QUOTIENT",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Numerator", ParamKind::Number), required("Denominator", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Returns the integer portion of a division, truncating the fractional part toward zero. \
         The result is an exact Integer; a zero denominator produces an error. MOD returns the \
         matching remainder.",
        &["QUOTIENT ( 7, 2 )", "QUOTIENT ( -7, 2 )"],
    ),
    spec(
        "RADIANS",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Converts an angle from degrees to radians by multiplying by pi/180. The result is an \
         approximate Decimal value; DEGREES performs the inverse conversion.",
        &["RADIANS ( 180 )"],
    ),
    spec(
        "RAND",
        FunctionCategory::MathAndTrig,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Decimal,
        FunctionFlags::new(false, true, false, true, true),
        "Returns a random Decimal value greater than or equal to 0 and less than 1, uniformly \
         distributed. RAND is volatile: it may return a different value on every evaluation and \
         changes on recalculation, so it cannot be used where a stable result is required.",
        &["RAND ()"],
    ),
    spec(
        "RANDBETWEEN",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Bottom", ParamKind::Number), required("Top", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        FunctionFlags::new(false, true, false, true, true),
        "Returns a random Integer between Bottom and Top inclusive. Both bounds are integers; \
         if Bottom is greater than Top the function raises an error. RANDBETWEEN is volatile, so \
         it recalculates to a different value and is unsuitable for stable results.",
        &["RANDBETWEEN ( 1, 10 )"],
    ),
    spec(
        "ROUND",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("NumberOfDigits", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Rounds a number to the specified number of digits, using half away from zero. A positive \
         count rounds right of the decimal point, zero rounds to an integer and a negative count \
         rounds left of it. The result is a Decimal, so large integers can lose precision beyond \
         about 15 significant digits.",
        &["ROUND ( 3.14159, 2 )", "ROUND ( 1234, -2 )"],
    ),
    spec(
        "ROUNDDOWN",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("NumberOfDigits", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Rounds a number down toward zero to the specified number of digits. Positive counts \
         work right of the decimal point, zero rounds to an integer and negative counts work \
         left of it. The result is a Decimal; ROUNDDOWN differs from FLOOR for negative numbers.",
        &["ROUNDDOWN ( 3.14159, 2 )", "ROUNDDOWN ( -3.14159, 2 )"],
    ),
    spec(
        "ROUNDUP",
        FunctionCategory::MathAndTrig,
        2,
        2,
        &[required("Number", ParamKind::Number), required("NumberOfDigits", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Rounds a number up away from zero to the specified number of digits. Positive counts \
         work right of the decimal point, zero rounds to an integer and negative counts work \
         left of it. The result is a Decimal.",
        &["ROUNDUP ( 3.14159, 2 )", "ROUNDUP ( -3.14159, 2 )"],
    ),
    spec(
        "SIGN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "Returns the sign of a real number: 1 when positive, 0 when zero and -1 when negative. \
         The result is an exact Integer; infinite values are not supported.",
        &["SIGN ( -5 )", "SIGN ( 0 )"],
    ),
    spec(
        "SIN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the sine of an angle given in radians. Convert degrees with RADIANS or by \
         multiplying by PI()/180; the result is an approximate Decimal value.",
        &["SIN ( PI () / 2 )"],
    ),
    spec(
        "SINH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the hyperbolic sine of a real number. The result is an approximate Decimal \
         value; ASINH is its inverse.",
        &["SINH ( 1 )"],
    ),
    spec(
        "SQRT",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the square root of a non-negative number as an approximate Decimal value. A \
         negative argument raises an error; SQRTPI multiplies by pi before taking the root.",
        &["SQRT ( 16 )"],
    ),
    spec(
        "SQRTPI",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the square root of Number multiplied by pi, as an approximate Decimal value. \
         Negative arguments produce an error; SQRTPI(n) equals SQRT(n * PI()).",
        &["SQRTPI ( 2 )"],
    ),
    spec(
        "TAN",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the tangent of an angle given in radians. Convert degrees with RADIANS or by \
         multiplying by PI()/180; the result is an approximate Decimal value and is undefined at \
         odd multiples of pi/2.",
        &["TAN ( PI () / 4 )"],
    ),
    spec(
        "TANH",
        FunctionCategory::MathAndTrig,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Returns the hyperbolic tangent of a real number, computed as SINH(number)/COSH(number). \
         The result is an approximate Decimal value between -1 and 1.",
        &["TANH ( 1 )"],
    ),
    spec(
        "TRUNC",
        FunctionCategory::MathAndTrig,
        1,
        2,
        &[required("Number", ParamKind::Number), optional("NumberOfDigits", ParamKind::Number)],
        &[overload(&[ParamKind::Number]), overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "Truncates a number toward zero by removing its fractional part or the digits beyond the \
         requested precision. NumberOfDigits defaults to 0, positive counts keep decimal places \
         and negative counts truncate left of the decimal point. The result is always Decimal, \
         unlike INT, which rounds toward negative infinity and follows the input type.",
        &["TRUNC ( 3.14159 )", "TRUNC ( 3.14159, 2 )"],
    ),
];
