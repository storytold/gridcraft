//! Information functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The `INFO.*` records take no required arguments: dax.guide documents an optional, repeatable
//! `RestrictionName` / `RestrictionValue` pair for every one of them, so their arity is open and
//! the descriptive signature keeps both positions optional. The `INFO.VIEW.*` functions take no
//! arguments at all.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, pairs, required, spec, variadic};

/// An `INFO.*` table function with the shared optional restriction-pair syntax.
///
/// The restrictions are name/value pairs, so only an even argument count is offered: an INFO
/// evaluator has no way to apply a dangling name, which is why the catalog carries no
/// one-`Text` overload for it.
macro_rules! info {
    ($name:literal, $description:literal) => {
        spec(
            $name,
            FunctionCategory::Information,
            0,
            usize::MAX,
            &[optional("RestrictionName", ParamKind::Text), optional("RestrictionValue", ParamKind::Any)],
            &[overload(&[]), pairs(&[ParamKind::Text, ParamKind::Any])],
            ReturnType::Table,
            FunctionFlags::new(false, false, false, false, false),
            $description,
            &[concat!($name, " ()")],
        )
    };
}

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "COLUMNSTATISTICS",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Returns one row per column visible in the model, with the table name, column name, \
         minimum, maximum, cardinality and maximum string length. The statistics respect the \
         security roles of the connected user.",
        &["COLUMNSTATISTICS ()"],
    ),
    spec(
        "CONTAINS",
        FunctionCategory::Information,
        3,
        usize::MAX,
        &[required("Table", ParamKind::Table), required("ColumnName", ParamKind::Column), required("Value", ParamKind::Any)],
        &[variadic(&[ParamKind::Table, ParamKind::Column, ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when at least one row of the table has all the supplied column/value \
         pairs. Each pair is a column reference followed by the value to match, using strict \
         equality (BLANK does not match 0).",
        &["CONTAINS ( Product, Product[Color], \"Red\" )", "CONTAINS ( Sales, Sales[ProductKey], 1, Sales[Quantity], 2 )"],
    ),
    spec(
        "CONTAINSROW",
        FunctionCategory::Information,
        2,
        usize::MAX,
        &[required("Table", ParamKind::Table), required("Value", ParamKind::Any)],
        &[variadic(&[ParamKind::Table, ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the table has a row whose values equal every supplied value, in \
         column order. The IN operator executes this function; comparisons are strict, so \
         BLANK does not match 0.",
        // The first argument is the row to test against, so both examples pair a one-column table
        // (a constructor, or a table expression) with exactly one scalar value.
        &["CONTAINSROW ( { \"Red\", \"Blue\" }, \"Red\" )", "CONTAINSROW ( VALUES ( Sales[ProductKey] ), 1 )"],
    ),
    spec(
        "CONTAINSSTRING",
        FunctionCategory::Information,
        2,
        2,
        &[required("WithinText", ParamKind::Text), required("FindText", ParamKind::Text)],
        &[overload(&[ParamKind::Text, ParamKind::Text])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when WithinText contains FindText. The comparison is case-insensitive and \
         accent-sensitive; `?` and `*` are wildcards and `~?` / `~*` match them literally.",
        &["CONTAINSSTRING ( Product[Product Name], \"red\" )", "CONTAINSSTRING ( \"Hello\", \"ell\" )"],
    ),
    spec(
        "CONTAINSSTRINGEXACT",
        FunctionCategory::Information,
        2,
        2,
        &[required("WithinText", ParamKind::Text), required("FindText", ParamKind::Text)],
        &[overload(&[ParamKind::Text, ParamKind::Text])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when WithinText contains FindText using a case-sensitive and \
         accent-sensitive comparison. Wildcards are not interpreted.",
        &["CONTAINSSTRINGEXACT ( Product[Product Name], \"Red\" )", "CONTAINSSTRINGEXACT ( \"Hello\", \"Hell\" )"],
    ),
    spec(
        "CUSTOMDATA",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, true, false, true, false),
        "Returns the CustomData connection-string property supplied by the client, or an empty \
         string when the connection did not define it. It does not inspect the data model or the \
         filter context. GridCraft has no client connection string in a workbook, so the \
         evaluator always answers the empty string.",
        &["CUSTOMDATA ()"],
    ),
    spec(
        "EVALUATEANDLOG",
        FunctionCategory::Information,
        1,
        3,
        &[required("Value", ParamKind::Any), optional("Label", ParamKind::Text), optional("MaxRows", ParamKind::Number)],
        &[overload(&[ParamKind::Any]), overload(&[ParamKind::Any, ParamKind::Text]), overload(&[ParamKind::Any, ParamKind::Text, ParamKind::Number])],
        ReturnType::Variant,
        FunctionFlags::new(false, false, false, false, false),
        "Returns the first argument unchanged and writes its value to the DAX Evaluation Log \
         trace event. The optional Label annotates the event; MaxRows caps the rows captured for \
         a table expression and defaults to 10.",
        &["EVALUATEANDLOG ( 1 )", "EVALUATEANDLOG ( [Sales Amount] )", "EVALUATEANDLOG ( Sales, \"Sales rows\", 5 )"],
    ),
    spec(
        "HASONEFILTER",
        FunctionCategory::Information,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the column has exactly one distinct value resulting from direct \
         filters on that column. Unlike HASONEVALUE it ignores filters propagated from related \
         tables.",
        &["HASONEFILTER ( Product[Color] )", "HASONEFILTER ( Sales[Quantity] )"],
    ),
    spec(
        "HASONEVALUE",
        FunctionCategory::Information,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the filter context leaves exactly one distinct value in the column, \
         regardless of whether the filter is direct or propagated through a relationship.",
        &["HASONEVALUE ( Product[Category] )", "HASONEVALUE ( Sales[Quantity] )"],
    ),
    info!(
        "INFO.ALTERNATEOFDEFINITIONS",
        "Exposes the TMSCHEMA_ALTERNATE_OF_DEFINITIONS data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.ANNOTATIONS",
        "One row per annotation in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.ATTRIBUTEHIERARCHIES",
        "Exposes the TMSCHEMA_ATTRIBUTE_HIERARCHIES schema rowset as a table; optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.ATTRIBUTEHIERARCHYSTORAGES",
        "Exposes the TMSCHEMA_ATTRIBUTE_HIERARCHY_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.CALCDEPENDENCY",
        "Reports which objects a DAX query depends on, so tools can inspect the dependency graph \
         without parsing formulas."
    ),
    info!(
        "INFO.CALCULATIONGROUPS",
        "Exposes the TMSCHEMA_CALCULATIONGROUPS data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.CALCULATIONITEMS",
        "Exposes the TMSCHEMA_CALCULATIONITEMS data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.CALENDARCOLUMNGROUPS",
        "Exposes the TMSCHEMA_CALENDAR_COLUMN_GROUPS schema rowset as a table; optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.CALENDARCOLUMNREFERENCES",
        "Exposes the TMSCHEMA_CALENDAR_COLUMN_REFERENCES schema rowset as a table; optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.CALENDARS",
        "Exposes the TMSCHEMA_CALENDARS schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.CATALOGS",
        "Exposes the DBSCHEMA_CATALOGS schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.CHANGEDPROPERTIES",
        "Exposes the TMSCHEMA_CHANGED_PROPERTIES schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.COLUMNPARTITIONSTORAGES",
        "Exposes the TMSCHEMA_COLUMN_PARTITION_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.COLUMNPERMISSIONS",
        "One row per column permission in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.COLUMNS",
        "One row per column in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.COLUMNSTORAGES",
        "One row per column storage in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.CSDLMETADATA",
        "Provides the model metadata serialized as CSDL XML text, so a tool can read the \
         conceptual schema without a client library."
    ),
    info!(
        "INFO.CULTURES",
        "One row per culture in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.DATACOVERAGEDEFINITIONS",
        "Exposes the TMSCHEMA_DATACOVERAGEDEFINITIONS data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.DATASOURCES",
        "Exposes the TMSCHEMA_DATASOURCES schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.DELTATABLEMETADATASTORAGES",
        "Exposes the TMSCHEMA_DELTA_TABLE_METADATA_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.DEPENDENCIES",
        "Returns the dependency records of a DAX query, describing what the query reads from the \
         model."
    ),
    info!(
        "INFO.DETAILROWSDEFINITIONS",
        "One row per detail-rows definition in the model, with the columns of the matching \
         schema rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.DICTIONARYSTORAGES",
        "Exposes the TMSCHEMA_DICTIONARY_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.EXCLUDEDARTIFACTS",
        "Exposes the TMSCHEMA_EXCLUDED_ARTIFACTS schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.EXPRESSIONS",
        "One row per expression in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.EXTENDEDPROPERTIES",
        "One row per extended property in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.FORMATSTRINGDEFINITIONS",
        "Exposes the TMSCHEMA_FORMAT_STRING_DEFINITIONS data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.FUNCTIONS",
        "Lists the DAX functions the engine makes available, one row per function. Optional \
         restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.GENERALSEGMENTMAPSEGMENTMETADATASTORAGES",
        "Exposes the TMSCHEMA_GENERAL_SEGMENT_MAP_SEGMENT_METADATA_STORAGES data management view \
         as a table; the optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.GROUPBYCOLUMNS",
        "Exposes the TMSCHEMA_GROUP_BY_COLUMNS data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.HIERARCHIES",
        "Exposes the TMSCHEMA_HIERARCHIES schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.HIERARCHYSTORAGES",
        "Exposes the TMSCHEMA_HIERARCHY_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.KPIS",
        "One row per KPI in the model, with the columns of the matching schema rowset. Optional \
         restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.LEVELS",
        "One row per level in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.LINGUISTICMETADATA",
        "Exposes the TMSCHEMA_LINGUISTIC_METADATA schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.MEASURES",
        "One row per measure in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.MODEL",
        "Exposes the TMSCHEMA_MODEL schema rowset as a table; optional restriction name/value \
         pairs filter its rows."
    ),
    info!(
        "INFO.OBJECTTRANSLATIONS",
        "One row per object translation in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PARQUETFILESTORAGES",
        "Exposes the TMSCHEMA_PARQUET_FILE_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.PARTITIONS",
        "Exposes the TMSCHEMA_PARTITIONS schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.PARTITIONSTORAGES",
        "One row per partition storage in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PERSPECTIVECOLUMNS",
        "One row per perspective column in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PERSPECTIVEHIERARCHIES",
        "One row per perspective hierarchy in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PERSPECTIVEMEASURES",
        "One row per perspective measure in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PERSPECTIVES",
        "One row per perspective in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PERSPECTIVETABLES",
        "One row per perspective table in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.PROPERTIES",
        "Exposes the DISCOVER_PROPERTIES schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.QUERYGROUPS",
        "Exposes the TMSCHEMA_QUERY_GROUPS data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.REFRESHPOLICIES",
        "Exposes the TMSCHEMA_REFRESH_POLICIES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.RELATEDCOLUMNDETAILS",
        "Exposes the TMSCHEMA_RELATED_COLUMN_DETAILS data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.RELATIONSHIPINDEXSTORAGES",
        "Exposes the TMSCHEMA_RELATIONSHIP_INDEX_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.RELATIONSHIPS",
        "Exposes the TMSCHEMA_RELATIONSHIPS schema rowset as a table; optional restriction \
         name/value pairs filter its rows."
    ),
    info!(
        "INFO.RELATIONSHIPSTORAGES",
        "Exposes the TMSCHEMA_RELATIONSHIP_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.ROLEMEMBERSHIPS",
        "One row per role membership in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.ROLES",
        "One row per role in the model, with the columns of the matching schema rowset. Optional \
         restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.SEGMENTMAPSTORAGES",
        "One row per segment map storage in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.SEGMENTSTORAGES",
        "Exposes the TMSCHEMA_SEGMENT_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.STORAGEFILES",
        "One row per storage file in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.STORAGEFOLDERS",
        "One row per storage folder in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.STORAGETABLECOLUMNS",
        "Reports storage statistics for each column of the in-memory tables, so a tool can see \
         how the model is packed."
    ),
    info!(
        "INFO.STORAGETABLECOLUMNSEGMENTS",
        "Describes the column segments that hold data for the in-memory tables, one row per \
         segment."
    ),
    info!(
        "INFO.TABLEPERMISSIONS",
        "One row per table permission in the model, with the columns of the matching schema \
         rowset. Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.TABLES",
        "One row per table in the model, with the columns of the matching schema rowset. Optional \
         restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.TABLESTORAGES",
        "One row per table storage in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    info!(
        "INFO.USERDEFINEDFUNCTIONS",
        "Exposes the TMSCHEMA_FUNCTIONS schema rowset as a table; optional restriction name/value \
         pairs filter its rows."
    ),
    info!(
        "INFO.VARIATIONS",
        "One row per variation in the model, with the columns of the matching schema rowset. \
         Optional restriction name/value pairs filter the rows."
    ),
    spec(
        "INFO.VIEW.COLUMNS",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Table of every column in the model, decoded so a query can use it without joins to \
         other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
         calculated columns and calculated tables.",
        &["INFO.VIEW.COLUMNS ()"],
    ),
    spec(
        "INFO.VIEW.MEASURES",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Table of every measure in the model, decoded so a query can use it without joins to \
         other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
         calculated columns and calculated tables.",
        &["INFO.VIEW.MEASURES ()"],
    ),
    spec(
        "INFO.VIEW.RELATIONSHIPS",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Table of every relationship in the model, decoded so a query can use it without joins \
         to other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
         calculated columns and calculated tables.",
        &["INFO.VIEW.RELATIONSHIPS ()"],
    ),
    spec(
        "INFO.VIEW.TABLES",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Table of every table in the model, decoded so a query can use it without joins to other \
         DMV rowsets. Unlike the plain INFO functions it is allowed in measures, calculated \
         columns and calculated tables.",
        &["INFO.VIEW.TABLES ()"],
    ),
    spec(
        "ISAFTER",
        FunctionCategory::Information,
        2,
        usize::MAX,
        &[required("Value1", ParamKind::Any), required("Value2", ParamKind::Any), optional("Order", ParamKind::Logical)],
        &[variadic(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the Value1/Value2 groups compare strictly after the corresponding \
         values in the ascending/descending order given by each optional Order flag. The \
         comparison uses the existing evaluation context; ISAFTER is not an iterator.",
        &["ISAFTER ( Sales[Year], 2008 )", "ISAFTER ( Sales[Year], 2008, 1 )"],
    ),
    spec(
        "ISBLANK",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is BLANK. Zero and the empty string are not blank; the \
         predicate is equivalent to the strict comparison `value == BLANK ()`.",
        &["ISBLANK ( Sales[Amount] )", "ISBLANK ( BLANK () )"],
    ),
    spec(
        "ISBOOLEAN",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE for a Boolean value. Alias of ISLOGICAL.",
        &["ISBOOLEAN ( TRUE )", "ISBOOLEAN ( Sales[Amount] )"],
    ),
    spec(
        "ISCROSSFILTERED",
        FunctionCategory::Information,
        1,
        1,
        &[required("TableNameOrColumnName", ParamKind::Reference)],
        &[overload(&[ParamKind::Reference])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the table or column is cross-filtered, meaning a filter is applied to \
         that table, to one of its columns, or to a column of a related table.",
        &["ISCROSSFILTERED ( Product )", "ISCROSSFILTERED ( Product[Category] )"],
    ),
    spec(
        "ISDATETIME",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value has the Date/time type.",
        &["ISDATETIME ( DATE ( 2008, 1, 1 ) )", "ISDATETIME ( Sales[Order Date] )"],
    ),
    spec(
        "ISDECIMAL",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE for a fixed-precision decimal (Currency) value. Alias of ISCURRENCY.",
        &["ISDECIMAL ( 1.5 )", "ISDECIMAL ( Sales[Net Price] )"],
    ),
    spec(
        "ISDOUBLE",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE for a floating-point decimal value.",
        &["ISDOUBLE ( 1.5 )", "ISDOUBLE ( Sales[Amount] )"],
    ),
    spec(
        "ISEMPTY",
        FunctionCategory::Information,
        1,
        1,
        &[required("Table", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the table expression has no rows in the current evaluation context, \
         including tables whose only row would be blank.",
        &["ISEMPTY ( Sales )", "ISEMPTY ( FILTER ( Sales, FALSE ) )"],
    ),
    spec(
        "ISERROR",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when evaluating the value produces an error. It inspects the value, so the \
         error is caught rather than propagated.",
        &["ISERROR ( 1 / 0 )", "ISERROR ( Sales[Amount] )"],
    ),
    spec(
        "ISEVEN",
        FunctionCategory::Information,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the number is even, FALSE when it is odd. Truncation applies to \
         non-integers, matching the mathematical sign of the truncated value.",
        &["ISEVEN ( 4 )", "ISEVEN ( Sales[Quantity] )"],
    ),
    spec(
        "ISFILTERED",
        FunctionCategory::Information,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE when the column carries direct filters in the current context; filters propagated \
         from a related table do not count, so use ISCROSSFILTERED for those.",
        &["ISFILTERED ( Product[Color] )", "ISFILTERED ( Sales[Quantity] )"],
    ),
    spec(
        "ISINSCOPE",
        FunctionCategory::Information,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the column is the level currently in scope in a hierarchy of levels, \
         for example the group-by column of the current SUMMARIZECOLUMNS row.",
        &["ISINSCOPE ( Product[Category] )", "ISINSCOPE ( Product[Subcategory] )"],
    ),
    spec(
        "ISINT64",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is a 64-bit integer. This is the alias of ISINTEGER.",
        &["ISINT64 ( 1 )", "ISINT64 ( Sales[Quantity] )"],
    ),
    spec(
        "ISLOGICAL",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE for a logical value (TRUE or FALSE). Alias of ISBOOLEAN.",
        &["ISLOGICAL ( FALSE )", "ISLOGICAL ( Sales[Amount] )"],
    ),
    spec(
        "ISNONTEXT",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is not text. A blank cell is not text, so ISNONTEXT also \
         returns TRUE for BLANK.",
        &["ISNONTEXT ( 1 )", "ISNONTEXT ( Product[Color] )"],
    ),
    spec(
        "ISNUMBER",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is a number. Text that looks numeric is not a number; use \
         VALUE to convert it first.",
        &["ISNUMBER ( 1 )", "ISNUMBER ( Sales[Amount] )"],
    ),
    spec(
        "ISNUMERIC",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "TRUE for an integer or decimal number, including Currency and floating-point values.",
        &["ISNUMERIC ( 1.5 )", "ISNUMERIC ( Sales[Net Price] )"],
    ),
    spec(
        "ISODD",
        FunctionCategory::Information,
        1,
        1,
        &[required("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Number])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the number is odd, FALSE when it is even. Non-integer values are \
         truncated before the parity test.",
        &["ISODD ( 3 )", "ISODD ( Sales[Quantity] )"],
    ),
    spec(
        "ISONORAFTER",
        FunctionCategory::Information,
        2,
        usize::MAX,
        &[required("Value1", ParamKind::Any), required("Value2", ParamKind::Any), optional("Order", ParamKind::Logical)],
        &[variadic(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the Value1/Value2 groups compare on or after the corresponding values \
         in the order given by each optional Order flag, emulating a START AT clause. It is not \
         an iterator.",
        &["ISONORAFTER ( Sales[Year], 2008 )", "ISONORAFTER ( Sales[Year], 2008, 1 )"],
    ),
    spec(
        "ISSELECTEDMEASURE",
        FunctionCategory::Information,
        1,
        usize::MAX,
        &[required("Measure", ParamKind::Reference)],
        &[variadic(&[ParamKind::Reference])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, false, true),
        "Returns TRUE when the measure currently evaluated in a calculation item is one of the \
         listed measures. A query measure redefined in the DAX query is not recognised.",
        &["ISSELECTEDMEASURE ( [Sales Amount] )", "ISSELECTEDMEASURE ( [Sales Amount], [Sales Quantity] )"],
    ),
    spec(
        "ISSTRING",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is a string. This is the alias of ISTEXT; a blank value is \
         not text.",
        &["ISSTRING ( \"text\" )", "ISSTRING ( Product[Color] )"],
    ),
    spec(
        "ISSUBTOTAL",
        FunctionCategory::Information,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, true, true, true),
        "Returns TRUE when the current SUMMARIZE row is the subtotal for the column, FALSE for a \
         detail row. It only makes sense inside SUMMARIZE with ROLLUP; SUMMARIZECOLUMNS with \
         ROLLUPADDISSUBTOTAL supersedes it.",
        &["ISSUBTOTAL ( Product[Brand] )", "ISSUBTOTAL ( Product[Category] )"],
    ),
    spec(
        "ISTEXT",
        FunctionCategory::Information,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, true),
        "Returns TRUE when the value is text. This is the alias of ISSTRING; a blank value is not \
         text.",
        &["ISTEXT ( \"text\" )", "ISTEXT ( Product[Color] )"],
    ),
    spec(
        "NAMEOF",
        FunctionCategory::Information,
        1,
        3,
        &[required("Value", ParamKind::Reference), optional("Component", ParamKind::Text), optional("Escaped", ParamKind::Text)],
        &[
            overload(&[ParamKind::Reference]),
            overload(&[ParamKind::Reference, ParamKind::Text]),
            overload(&[ParamKind::Reference, ParamKind::Text, ParamKind::Text]),
        ],
        ReturnType::String,
        FunctionFlags::new(false, false, false, false, true),
        "Text name of a table, column, measure or calendar. Component selects TABLE, COLUMN, \
         MEASURE, CALENDAR, FULL, SELF or PARENT; Escaped selects ESCAPED, UNESCAPED or \
         MINIMALLYESCAPED. Variables and dynamic expressions are not accepted.",
        &["NAMEOF ( Sales[Amount] )", "NAMEOF ( [Sales Amount] )"],
    ),
    spec(
        "SELECTEDMEASURE",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, false, true),
        "Returns a reference to the measure currently evaluated in a calculation item. During \
         application each call is replaced by the selected measure, which also triggers context \
         transition.",
        &["SELECTEDMEASURE ()"],
    ),
    spec(
        "SELECTEDMEASUREFORMATSTRING",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, false, false, false, true),
        "Format string of the measure under evaluation in the current calculation item.",
        &["SELECTEDMEASUREFORMATSTRING ()"],
    ),
    spec(
        "SELECTEDMEASURENAME",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, false, false, false, true),
        "Name of the measure under evaluation in the current calculation item, including one \
         redefined in the DAX query.",
        &["SELECTEDMEASURENAME ()"],
    ),
    spec(
        "TABLEOF",
        FunctionCategory::Information,
        1,
        1,
        &[required("Reference", ParamKind::Reference)],
        &[overload(&[ParamKind::Reference])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Resolves a column, measure or calendar reference to the table that owns it.",
        &["TABLEOF ( Sales[Amount] )", "TABLEOF ( Product[Color] )"],
    ),
    spec(
        "TEXTCONTAINS",
        FunctionCategory::Information,
        2,
        3,
        &[required("WithinTextColumn", ParamKind::Column), required("SearchText", ParamKind::Text), optional("MatchMode", ParamKind::Text)],
        &[overload(&[ParamKind::Column, ParamKind::Text]), overload(&[ParamKind::Column, ParamKind::Text, ParamKind::Text])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, false, false),
        "TRUE when the column text matches SearchText under full-text query semantics. MatchMode \
         is TEXTMATCHING (the default), FUZZYMATCHING or PHRASEMATCHING.",
        &["TEXTCONTAINS ( Product[Product Name], \"red\" )", "TEXTCONTAINS ( Product[Product Name], \"red\", \"FUZZYMATCHING\" )"],
    ),
    spec(
        "TEXTSIMILARITY",
        FunctionCategory::Information,
        2,
        3,
        &[required("WithinTextColumn", ParamKind::Column), required("SearchText", ParamKind::Text), optional("MatchMode", ParamKind::Text)],
        &[overload(&[ParamKind::Column, ParamKind::Text]), overload(&[ParamKind::Column, ParamKind::Text, ParamKind::Text])],
        ReturnType::Decimal,
        FunctionFlags::new(false, false, false, false, false),
        "Returns the BM25 relevance score of the text in the column against the search text; a \
         higher score is a stronger match and 0 means no match. MatchMode is TEXTMATCHING (the \
         default), FUZZYMATCHING or PHRASEMATCHING. Scores are comparable only within the same \
         search context.",
        &["TEXTSIMILARITY ( Product[Product Name], \"red\" )", "TEXTSIMILARITY ( Product[Product Name], \"red\", \"FUZZYMATCHING\" )"],
    ),
    spec(
        "USERCULTURE",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, true, false, false, true),
        "Returns the culture code of the current user, such as \"en-US\", derived from the \
         operating system or browser settings.",
        &["USERCULTURE ()"],
    ),
    spec(
        "USERNAME",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, true, false, true, false),
        "The connection identity in domain\\user form, as supplied when the connection was \
         opened.",
        &["USERNAME ()"],
    ),
    spec(
        "USEROBJECTID",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, true, false, false, false),
        "Returns the current user's object ID from Azure AD, or the security identifier (SID) when \
         the model runs on-premises.",
        &["USEROBJECTID ()"],
    ),
    spec(
        "USERPRINCIPALNAME",
        FunctionCategory::Information,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::String,
        FunctionFlags::new(false, true, false, false, false),
        "Returns the user principal name of the current user, typically the email address in \
         user@domain form.",
        &["USERPRINCIPALNAME ()"],
    ),
];
