//! Parent-child functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! These five functions flatten a parent-child hierarchy into a delimited path. The delimiter
//! is always the vertical bar (`|`); `PATH` produces the string, `PATHLENGTH` counts its items,
//! `PATHCONTAINS` searches it, and `PATHITEM`/`PATHITEMREVERSE` extract one item from the left
//! or the right end.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, standard};

const FLAGS: FunctionFlags = standard(false);

/// dax.guide lists the parent-child functions in this order: PATH, PATHCONTAINS, PATHITEM,
/// PATHITEMREVERSE and PATHLENGTH.
pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "PATH",
        FunctionCategory::ParentChild,
        2,
        2,
        &[required("ID_ColumnName", ParamKind::Column), required("Parent_ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column, ParamKind::Column])],
        ReturnType::String,
        FLAGS,
        "Returns a text string containing the delimited list of IDs that starts at the top of a \
         parent-child hierarchy and ends with the current row, separating the identifiers with \
         the vertical bar. Both arguments must be column references, not expressions, and must \
         share a data type (integer or text); the parent value must exist in the ID column. A \
         blank parent returns just the ID, a blank ID returns BLANK, and an ID that contains a \
         vertical bar returns an error. Duplicate IDs with different parents return an error; \
         with the same parent the common parent is returned.",
        &["PATH ( Account[AccountKey], Account[ParentAccountKey] )", "PATH ( Employee[EmployeeKey], Employee[ParentEmployeeKey] )"],
    ),
    spec(
        "PATHCONTAINS",
        FunctionCategory::ParentChild,
        2,
        2,
        &[required("Path", ParamKind::Text), required("Item", ParamKind::Any)],
        &[overload(&[ParamKind::Text, ParamKind::Any])],
        ReturnType::Boolean,
        FLAGS,
        "Returns TRUE when the item appears as a whole segment of the delimited path, and FALSE \
         otherwise. The path is normally the result of PATH. An integer item is converted to \
         text before the comparison; when that conversion fails the function returns an error.",
        &["PATHCONTAINS ( Account[AccountPath], 2 )", "PATHCONTAINS ( Account[AccountPath], \"24\" )", "PATHCONTAINS ( \"1|24|2\", 2 )"],
    ),
    spec(
        "PATHITEM",
        FunctionCategory::ParentChild,
        2,
        3,
        &[required("Path", ParamKind::Text), required("Position", ParamKind::Number), optional("Type", ParamKind::Any)],
        &[overload(&[ParamKind::Text, ParamKind::Number]), overload(&[ParamKind::Text, ParamKind::Number, ParamKind::Any])],
        ReturnType::Any,
        FLAGS,
        "Returns the item at the given position in the delimited path, counting from the left \
         end where 1 is the top of the hierarchy. A position below 1 or beyond the number of \
         items returns BLANK. The optional Type selects the result: TEXT (or 0), the default, \
         returns a string, while INTEGER (or 1) converts the item to an integer. The result \
         type therefore follows the Type argument. An invalid Type enumeration raises an error.",
        &[
            "PATHITEM ( Account[AccountPath], 1 )",
            "PATHITEM ( Account[AccountPath], 2, INTEGER )",
            "PATHITEM ( Account[AccountPath], 3, TEXT )",
            "PATHITEM ( \"123|456|789|764\", 2 )",
        ],
    ),
    spec(
        "PATHITEMREVERSE",
        FunctionCategory::ParentChild,
        2,
        3,
        &[required("Path", ParamKind::Text), required("Position", ParamKind::Number), optional("Type", ParamKind::Any)],
        &[overload(&[ParamKind::Text, ParamKind::Number]), overload(&[ParamKind::Text, ParamKind::Number, ParamKind::Any])],
        ReturnType::Any,
        FLAGS,
        "Returns the item at the given position in the delimited path, counting backwards from \
         the last item, so position 1 is the current item and position 2 is its immediate \
         parent. A position below 1 or beyond the number of items returns BLANK. The optional \
         Type selects the result: TEXT (or 0), the default, returns a string, while INTEGER (or \
         1) converts the item to an integer, so the result type follows the Type argument. An \
         invalid Type enumeration raises an error.",
        &[
            "PATHITEMREVERSE ( Account[AccountPath], 1 )",
            "PATHITEMREVERSE ( Account[AccountPath], 2, INTEGER )",
            "PATHITEMREVERSE ( Account[AccountPath], 3, TEXT )",
            "PATHITEMREVERSE ( \"123|456|789|764\", 2 )",
        ],
    ),
    spec(
        "PATHLENGTH",
        FunctionCategory::ParentChild,
        1,
        1,
        &[required("Path", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::Integer,
        FLAGS,
        "Returns the number of items in a delimited path, including the current item, so the path \
         of a root node has length 1. The path is normally the result of PATH, but any text with \
         vertical-bar separators (including a literal such as \"123|456|789|764\") is accepted.",
        &["PATHLENGTH ( Account[AccountPath] )", "PATHLENGTH ( \"123|456|789|764\" )"],
    ),
];
