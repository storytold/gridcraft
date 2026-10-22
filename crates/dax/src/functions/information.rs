//! Information functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The `INFO.*` records take no required arguments: dax.guide documents an optional, repeatable
//! `RestrictionName` / `RestrictionValue` pair for every one of them, so their arity is open and
//! the descriptive signature keeps both positions optional. The `INFO.VIEW.*` functions take no
//! arguments at all.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// An `INFO.*` table function with the shared optional restriction-pair syntax.
macro_rules! info {
    ($name:literal, $description:literal) => {
        spec(
            $name,
            FunctionCategory::Information,
            0,
            usize::MAX,
            &[optional("RestrictionName", ParamKind::Text), optional("RestrictionValue", ParamKind::Any)],
            &[overload(&[]), overload(&[ParamKind::Text]), variadic(&[ParamKind::Text, ParamKind::Any])],
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
        &["CONTAINSROW ( { \"Red\", \"Blue\" }, Product[Color] )", "CONTAINSROW ( Sales, Sales[ProductKey], 1 )"],
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
        "Returns the CustomData connection-string property supplied by the client, or BLANK when \
         the connection did not define it. It does not inspect the data model or the filter \
         context.",
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
        "Returns a list of all annotations in the current model with columns matching the schema \
         rowset for annotation objects."
    ),
    info!("INFO.ATTRIBUTEHIERARCHIES", "Represents the TMSCHEMA_ATTRIBUTE_HIERARCHIES DMV query function."),
    info!(
        "INFO.ATTRIBUTEHIERARCHYSTORAGES",
        "Exposes the TMSCHEMA_ATTRIBUTE_HIERARCHY_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!("INFO.CALCDEPENDENCY", "Returns information about the calculation dependency for a DAX query."),
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
    info!("INFO.CALENDARCOLUMNGROUPS", "Represents the TMSCHEMA_CALENDAR_COLUMN_GROUPS DMV query function."),
    info!("INFO.CALENDARCOLUMNREFERENCES", "Represents the TMSCHEMA_CALENDAR_COLUMN_REFERENCES DMV query function."),
    info!("INFO.CALENDARS", "Represents the TMSCHEMA_CALENDARS DMV query function."),
    info!("INFO.CATALOGS", "Represents the DBSCHEMA_CATALOGS DMV query function."),
    info!("INFO.CHANGEDPROPERTIES", "Represents the TMSCHEMA_CHANGED_PROPERTIES DMV query function."),
    info!(
        "INFO.COLUMNPARTITIONSTORAGES",
        "Exposes the TMSCHEMA_COLUMN_PARTITION_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.COLUMNPERMISSIONS",
        "Returns a list of all column permissions in the current model with columns matching the \
         schema rowset for column permissions objects."
    ),
    info!(
        "INFO.COLUMNS",
        "Returns a list of all columns in the current model with columns matching the schema \
         rowset for column objects."
    ),
    info!(
        "INFO.COLUMNSTORAGES",
        "Returns a list of all column storages in the current model with columns matching the \
         schema rowset for column storage objects."
    ),
    info!("INFO.CSDLMETADATA", "Returns information about database metadata in XML format."),
    info!(
        "INFO.CULTURES",
        "Returns a list of all cultures in the current model with columns matching the schema \
         rowset for culture objects."
    ),
    info!(
        "INFO.DATACOVERAGEDEFINITIONS",
        "Exposes the TMSCHEMA_DATACOVERAGEDEFINITIONS data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!("INFO.DATASOURCES", "Represents the TMSCHEMA_DATASOURCES DMV query function."),
    info!(
        "INFO.DELTATABLEMETADATASTORAGES",
        "Exposes the TMSCHEMA_DELTA_TABLE_METADATA_STORAGES data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!("INFO.DEPENDENCIES", "Returns information about the calculation dependency for a DAX query."),
    info!(
        "INFO.DETAILROWSDEFINITIONS",
        "Returns a list of all detail rows definitions in the current model with columns matching \
         the schema rowset for detail rows definitions objects."
    ),
    info!(
        "INFO.DICTIONARYSTORAGES",
        "Exposes the TMSCHEMA_DICTIONARY_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!("INFO.EXCLUDEDARTIFACTS", "Represents the TMSCHEMA_EXCLUDED_ARTIFACTS DMV query function."),
    info!(
        "INFO.EXPRESSIONS",
        "Returns a list of all expressions in the current model with columns matching the schema \
         rowset for expressions objects."
    ),
    info!(
        "INFO.EXTENDEDPROPERTIES",
        "Returns a list of all extended properties in the current model with columns matching the \
         schema rowset for extended properties objects."
    ),
    info!(
        "INFO.FORMATSTRINGDEFINITIONS",
        "Exposes the TMSCHEMA_FORMAT_STRING_DEFINITIONS data management view as a table; the \
         optional restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.FUNCTIONS",
        "Returns information about the functions that are currently available for use in the DAX \
         programming language."
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
    info!("INFO.HIERARCHIES", "Represents the TMSCHEMA_HIERARCHIES DMV query function."),
    info!(
        "INFO.HIERARCHYSTORAGES",
        "Exposes the TMSCHEMA_HIERARCHY_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.KPIS",
        "Returns a list of all KPIs in the current model with columns matching the schema rowset \
         for KPI objects."
    ),
    info!(
        "INFO.LEVELS",
        "Returns a list of all levels in the current model with columns matching the schema \
         rowset for level objects."
    ),
    info!("INFO.LINGUISTICMETADATA", "Represents the TMSCHEMA_LINGUISTIC_METADATA DMV query function."),
    info!(
        "INFO.MEASURES",
        "Returns a list of all measures in the current model with columns matching the schema \
         rowset for measure objects."
    ),
    info!("INFO.MODEL", "Represents the TMSCHEMA_MODEL DMV query function."),
    info!(
        "INFO.OBJECTTRANSLATIONS",
        "Returns a list of all object translations in the current model with columns matching the \
         schema rowset for object translation objects."
    ),
    info!(
        "INFO.PARQUETFILESTORAGES",
        "Exposes the TMSCHEMA_PARQUET_FILE_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!("INFO.PARTITIONS", "Represents the TMSCHEMA_PARTITIONS DMV query function."),
    info!(
        "INFO.PARTITIONSTORAGES",
        "Returns a list of all partition storages in the current model with columns matching the \
         schema rowset for partition storage objects."
    ),
    info!(
        "INFO.PERSPECTIVECOLUMNS",
        "Returns a list of all perspective columns in the current model with columns matching the \
         schema rowset for perspective columns objects."
    ),
    info!(
        "INFO.PERSPECTIVEHIERARCHIES",
        "Returns a list of all perspective hierarchies in the current model with columns matching \
         the schema rowset for perspective hierarchies objects."
    ),
    info!(
        "INFO.PERSPECTIVEMEASURES",
        "Returns a list of all perspective measures in the current model with columns matching the \
         schema rowset for perspective measures objects."
    ),
    info!(
        "INFO.PERSPECTIVES",
        "Returns a list of all perspectives in the current model with columns matching the schema \
         rowset for perspectives objects."
    ),
    info!(
        "INFO.PERSPECTIVETABLES",
        "Returns a list of all perspective tables in the current model with columns matching the \
         schema rowset for perspective tables objects."
    ),
    info!("INFO.PROPERTIES", "Represents the DISCOVER_PROPERTIES DMV query function."),
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
    info!("INFO.RELATIONSHIPS", "Represents the TMSCHEMA_RELATIONSHIPS DMV query function."),
    info!(
        "INFO.RELATIONSHIPSTORAGES",
        "Exposes the TMSCHEMA_RELATIONSHIP_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.ROLEMEMBERSHIPS",
        "Returns a list of all role memberships in the current model with columns matching the \
         schema rowset for role memberships objects."
    ),
    info!(
        "INFO.ROLES",
        "Returns a list of all roles in the current model with columns matching the schema rowset \
         for roles objects."
    ),
    info!(
        "INFO.SEGMENTMAPSTORAGES",
        "Returns a list of all segment map storages in the current model with columns matching the \
         schema rowset for segment map storage objects."
    ),
    info!(
        "INFO.SEGMENTSTORAGES",
        "Exposes the TMSCHEMA_SEGMENT_STORAGES data management view as a table; the optional \
         restriction name/value pairs filter its rows."
    ),
    info!(
        "INFO.STORAGEFILES",
        "Returns a list of all storage files in the current model with columns matching the schema \
         rowset for storage file objects."
    ),
    info!(
        "INFO.STORAGEFOLDERS",
        "Returns a list of all storage folders in the current model with columns matching the \
         schema rowset for storage folder objects."
    ),
    info!("INFO.STORAGETABLECOLUMNS", "Returns statistics about the columns of in-memory tables."),
    info!("INFO.STORAGETABLECOLUMNSEGMENTS", "Returns information about the column segments used for storing data for in-memory tables."),
    info!(
        "INFO.TABLEPERMISSIONS",
        "Returns a list of all table permissions in the current model with columns matching the \
         schema rowset for table permissions objects."
    ),
    info!(
        "INFO.TABLES",
        "Returns a list of all tables in the current model with columns matching the schema rowset \
         for table objects."
    ),
    info!(
        "INFO.TABLESTORAGES",
        "Returns a list of all table storages in the current model with columns matching the \
         schema rowset for table storage objects."
    ),
    info!("INFO.USERDEFINEDFUNCTIONS", "Represents the TMSCHEMA_FUNCTIONS DMV query function."),
    info!(
        "INFO.VARIATIONS",
        "Returns a list of all variations in the current model with columns matching the schema \
         rowset for variations objects."
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
        "Returns a list of all columns in the current model, decoded for direct use without joins \
         to other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
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
        "Returns a list of all measures in the current model, decoded for direct use without joins \
         to other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
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
        "Returns a list of all relationships in the current model, decoded for direct use without \
         joins to other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
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
        "Returns a list of all tables in the current model, decoded for direct use without joins \
         to other DMV rowsets. Unlike the plain INFO functions it is allowed in measures, \
         calculated columns and calculated tables.",
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
        "Returns TRUE when the value is a Boolean (TRUE or FALSE). This is the alias of \
         ISLOGICAL.",
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
        "Returns TRUE when the value is a fixed-precision decimal number (Currency). This is the \
         alias of ISCURRENCY.",
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
        "Returns TRUE when the value is a floating-point decimal number.",
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
        "Returns TRUE when there are direct filters on the column. Filters propagated from a \
         related table do not count; use ISCROSSFILTERED for those.",
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
        "Returns TRUE when the value is a logical value (TRUE or FALSE). This is the alias of \
         ISBOOLEAN.",
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
        "Returns TRUE when the value is an integer or decimal number, including currency and \
         floating-point values.",
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
        FunctionFlags::new(false, true, false, false, true),
        "Returns the name of a table, column, measure or calendar as text. Component selects \
         TABLE, COLUMN, MEASURE, CALENDAR, FULL, SELF or PARENT; Escaped selects ESCAPED, \
         UNESCAPED or MINIMALLYESCAPED. Variables and dynamic expressions are not accepted.",
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
        "Returns the format string of the measure currently evaluated in a calculation item.",
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
        "Returns the name of the measure currently evaluated in a calculation item, including a \
         measure redefined in the DAX query.",
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
        "Returns the table that contains the specified column, measure or calendar reference.",
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
        "Returns TRUE when the text in the column matches the search text using full-text query \
         semantics. MatchMode is TEXTMATCHING (the default), FUZZYMATCHING or PHRASEMATCHING.",
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
        "Returns the domain name and user name of the current connection in domain\\user form, as \
         supplied at connection time.",
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
