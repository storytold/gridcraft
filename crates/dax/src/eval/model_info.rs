//! `INFO.*` metadata table functions.
//!
//! Every function exposes the columns of its documented `TMSCHEMA_*`/DMV rowset. Tables backed by
//! [`DataModel`] are populated from the model; model objects the in-memory representation cannot
//! express (storage, segments, perspectives, roles, permissions, query groups, ...) return the
//! documented columns with no rows.
//!
//! Two mappings are specific to GridCraft's model:
//! - `INFO.MODEL` describes the single in-memory model (`Name` is `Model`).
//! - `INFO.EXPRESSIONS` lists measure formulas, because [`DataModel`] has no Power Query
//!   expressions; `Kind` is the model-expression value `1`.
//!
//! Restriction name/value pairs are accepted (the catalog types them as an open argument list)
//! but not applied: the evaluator has a single complete model and no DMV restriction engine.

use crate::functions::catalog;
use crate::model::{DataModel, Relationship};

use super::*;

/// Identifier of the one in-memory model, shared by every row that points at a model.
const MODEL_ID: usize = 1;
/// Name used for the in-memory model in `INFO.MODEL` and the decoded `INFO.VIEW.*` tables.
const MODEL_NAME: &str = "Model";
/// `State` of ready model objects.
const STATE_READY: usize = 1;

fn schema(columns: &[&str]) -> Vec<TableColumn> {
    columns.iter().map(|name| TableColumn { name: (*name).into() }).collect()
}

/// Builds a table with the given documented columns.
fn table(columns: &[&str], rows: Vec<Vec<Scalar>>) -> TableValue {
    TableValue::new(schema(columns), rows)
}

/// Builds a documented table that the model cannot populate.
fn empty(columns: &[&str]) -> TableValue {
    table(columns, Vec::new())
}

fn text(value: impl Into<String>) -> Scalar {
    Scalar::String(value.into())
}

fn number(value: usize) -> Scalar {
    Scalar::Integer(value as i64)
}

fn flag(value: bool) -> Scalar {
    Scalar::Boolean(value)
}

fn blank() -> Scalar {
    Scalar::Blank
}

fn optional(value: &Option<String>) -> Scalar {
    value.as_deref().map_or(Scalar::Blank, text)
}

/// Global (model-wide) column identifiers, in table/column order; both `INFO.COLUMNS` and the
/// relationship rows use them.
fn column_ids(model: &DataModel) -> Vec<Vec<usize>> {
    let mut next = 1usize;
    model
        .tables
        .iter()
        .map(|table| {
            table
                .columns
                .iter()
                .map(|_| {
                    let id = next;
                    next += 1;
                    id
                })
                .collect()
        })
        .collect()
}

fn table_id(model: &DataModel, name: &str) -> Scalar {
    model.table_index(name).map_or(Scalar::Blank, |index| number(index + 1))
}

fn relationship_name(relationship: &Relationship) -> String {
    format!("{}[{}] -> {}[{}]", relationship.from_table, relationship.from_column, relationship.to_table, relationship.to_column)
}

/// `CrossFilteringBehavior`/`SecurityFilteringBehavior`: 1 = one direction, 2 = both.
fn filtering_behavior(bidirectional: bool) -> usize {
    if bidirectional { 2 } else { 1 }
}

fn filtering_behavior_text(bidirectional: bool) -> &'static str {
    if bidirectional { "BothDirections" } else { "OneDirection" }
}

fn tables(model: &DataModel) -> TableValue {
    let rows = model
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            vec![
                number(index + 1),
                number(MODEL_ID),
                text(&table.name),
                blank(),
                blank(),
                flag(false),
                blank(),
                blank(),
                blank(),
                number(0),
                flag(false),
                flag(false),
                blank(),
                number(0),
                blank(),
                blank(),
                flag(false),
                blank(),
                blank(),
                flag(false),
                flag(false),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "ModelID",
            "Name",
            "DataCategory",
            "Description",
            "IsHidden",
            "TableStorageID",
            "ModifiedTime",
            "StructureModifiedTime",
            "SystemFlags",
            "ShowAsVariationsOnly",
            "IsPrivate",
            "DefaultDetailRowsDefinitionID",
            "AlternateSourcePrecedence",
            "RefreshPolicyID",
            "CalculationGroupID",
            "ExcludeFromModelRefresh",
            "LineageTag",
            "SourceLineageTag",
            "SystemManaged",
            "ExcludeFromAutomaticAggregations",
        ],
        rows,
    )
}

fn columns(model: &DataModel) -> TableValue {
    let ids = column_ids(model);
    let mut rows = Vec::new();
    for (table_index, table) in model.tables.iter().enumerate() {
        for (column_index, column) in table.columns.iter().enumerate() {
            let data_type = optional(&column.data_type);
            rows.push(vec![
                number(ids[table_index][column_index]),
                number(table_index + 1),
                text(&column.name),
                text(&column.name),
                data_type.clone(),
                data_type,
                blank(),
                blank(),
                flag(false),
                number(STATE_READY),
                flag(false),
                flag(false),
                flag(true),
                blank(),
                blank(),
                flag(false),
                flag(false),
                number(0),
                blank(),
                number(1),
                text(&column.name),
                blank(),
                blank(),
                blank(),
                flag(true),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                number(0),
                flag(false),
                number(column_index + 1),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]);
        }
    }
    table(
        &[
            "ID",
            "TableID",
            "ExplicitName",
            "InferredName",
            "ExplicitDataType",
            "InferredDataType",
            "DataCategory",
            "Description",
            "IsHidden",
            "State",
            "IsUnique",
            "IsKey",
            "IsNullable",
            "Alignment",
            "TableDetailPosition",
            "IsDefaultLabel",
            "IsDefaultImage",
            "SummarizeBy",
            "ColumnStorageID",
            "Type",
            "SourceColumn",
            "ColumnOriginID",
            "Expression",
            "FormatString",
            "IsAvailableInMDX",
            "SortByColumnID",
            "AttributeHierarchyID",
            "ModifiedTime",
            "StructureModifiedTime",
            "RefreshedTime",
            "SystemFlags",
            "KeepUniqueRows",
            "DisplayOrdinal",
            "ErrorMessage",
            "SourceProviderType",
            "DisplayFolder",
            "EncodingHint",
            "RelatedColumnDetailsID",
            "AlternateOfID",
            "LineageTag",
            "SourceLineageTag",
        ],
        rows,
    )
}

fn measures(model: &DataModel) -> TableValue {
    let rows = model
        .measures
        .iter()
        .enumerate()
        .map(|(index, measure)| {
            let home_table = measure.table.as_deref().map_or(Scalar::Blank, |name| table_id(model, name));
            vec![
                number(index + 1),
                home_table,
                text(&measure.name),
                blank(),
                blank(),
                text(&measure.formula),
                optional(&measure.format),
                flag(false),
                number(STATE_READY),
                blank(),
                blank(),
                blank(),
                flag(false),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "TableID",
            "Name",
            "Description",
            "DataType",
            "Expression",
            "FormatString",
            "IsHidden",
            "State",
            "ModifiedTime",
            "StructureModifiedTime",
            "KPIID",
            "IsSimpleMeasure",
            "ErrorMessage",
            "DisplayFolder",
            "DetailRowsDefinitionID",
            "DataCategory",
            "LineageTag",
            "SourceLineageTag",
        ],
        rows,
    )
}

fn relationships(model: &DataModel) -> TableValue {
    let ids = column_ids(model);
    let rows = model
        .relationships
        .iter()
        .enumerate()
        .map(|(index, relationship)| {
            let from_table = model.table_index(&relationship.from_table);
            let from_column = from_table.and_then(|table| model.column_index(table, &relationship.from_column));
            let to_table = model.table_index(&relationship.to_table);
            let to_column = to_table.and_then(|table| model.column_index(table, &relationship.to_column));
            let from_column_id = match (from_table, from_column) {
                (Some(table), Some(column)) => number(ids[table][column]),
                _ => blank(),
            };
            let to_column_id = match (to_table, to_column) {
                (Some(table), Some(column)) => number(ids[table][column]),
                _ => blank(),
            };
            vec![
                number(index + 1),
                number(MODEL_ID),
                text(relationship_name(relationship)),
                flag(!relationship.inactive),
                number(1),
                number(filtering_behavior(relationship.bidirectional)),
                blank(),
                flag(false),
                from_table.map_or(Scalar::Blank, |table| number(table + 1)),
                from_column_id,
                number(2),
                to_table.map_or(Scalar::Blank, |table| number(table + 1)),
                to_column_id,
                number(1),
                number(STATE_READY),
                blank(),
                blank(),
                blank(),
                blank(),
                number(filtering_behavior(relationship.bidirectional)),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "ModelID",
            "Name",
            "IsActive",
            "Type",
            "CrossFilteringBehavior",
            "JoinOnDateBehavior",
            "RelyOnReferentialIntegrity",
            "FromTableID",
            "FromColumnID",
            "FromCardinality",
            "ToTableID",
            "ToColumnID",
            "ToCardinality",
            "State",
            "RelationshipStorageID",
            "RelationshipStorage2ID",
            "ModifiedTime",
            "RefreshedTime",
            "SecurityFilteringBehavior",
        ],
        rows,
    )
}

fn partitions(model: &DataModel) -> TableValue {
    let rows = model
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            vec![
                number(index + 1),
                number(index + 1),
                text(&table.name),
                blank(),
                blank(),
                blank(),
                number(STATE_READY),
                number(1),
                blank(),
                number(1),
                blank(),
                blank(),
                blank(),
                number(0),
                blank(),
                flag(false),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "TableID",
            "Name",
            "Description",
            "DataSourceID",
            "QueryDefinition",
            "State",
            "Type",
            "PartitionStorageID",
            "Mode",
            "DataView",
            "ModifiedTime",
            "RefreshedTime",
            "SystemFlags",
            "ErrorMessage",
            "RetainDataTillForceCalculate",
            "RangeStart",
            "RangeEnd",
            "RangeGranularity",
            "RefreshBookmark",
            "QueryGroupID",
            "ExpressionSourceID",
            "MAttributes",
            "DataCoverageDefinitionID",
            "SchemaName",
        ],
        rows,
    )
}

fn expressions(model: &DataModel) -> TableValue {
    let rows = model
        .measures
        .iter()
        .enumerate()
        .map(|(index, measure)| {
            vec![
                number(index + 1),
                number(MODEL_ID),
                text(&measure.name),
                text("Measure formula"),
                number(1),
                text(&measure.formula),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "ModelID",
            "Name",
            "Description",
            "Kind",
            "Expression",
            "ModifiedTime",
            "QueryGroupID",
            "ParameterValuesColumnID",
            "MAttributes",
            "LineageTag",
            "SourceLineageTag",
            "RemoteParameterName",
            "ExpressionSourceID",
        ],
        rows,
    )
}

fn parameter_list(spec: &crate::functions::FunctionSpec) -> String {
    spec.params
        .iter()
        .map(|parameter| if parameter.optional { format!("[{}]", parameter.name) } else { parameter.name.to_string() })
        .collect::<Vec<_>>()
        .join(", ")
}

fn functions() -> TableValue {
    let rows = catalog()
        .iter()
        .map(|spec| {
            vec![
                text(spec.name),
                text(spec.description),
                text(parameter_list(spec)),
                text(spec.return_type.name()),
                number(1),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                flag(!spec.flags.direct_query_limited),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "FUNCTION_NAME",
            "DESCRIPTION",
            "PARAMETER_LIST",
            "RETURN_TYPE",
            "ORIGIN",
            "INTERFACE_NAME",
            "LIBRARY_NAME",
            "DLL_NAME",
            "HELP_FILE",
            "HELP_CONTEXT",
            "OBJECT",
            "CAPTION",
            "PARAMETERINFO",
            "DIRECTQUERY_PUSHABLE",
            "VISUAL_CALCULATIONS_INFO",
        ],
        rows,
    )
}

fn model_table() -> TableValue {
    let row = vec![
        number(1),
        text(MODEL_NAME),
        blank(),
        blank(),
        number(1),
        number(1),
        blank(),
        blank(),
        blank(),
        blank(),
        number(1),
        blank(),
        blank(),
        blank(),
        flag(false),
        flag(true),
        blank(),
        number(0),
        blank(),
        blank(),
        flag(false),
        blank(),
        flag(false),
        number(0),
        number(0),
        blank(),
        blank(),
    ];
    table(
        &[
            "ID",
            "Name",
            "Description",
            "StorageLocation",
            "DefaultMode",
            "DefaultDataView",
            "Culture",
            "Collation",
            "ModifiedTime",
            "StructureModifiedTime",
            "Version",
            "DataAccessOptions",
            "DefaultMeasureID",
            "DefaultPowerBIDataSourceVersion",
            "ForceUniqueNames",
            "DiscourageImplicitMeasures",
            "DataSourceVariablesOverrideBehavior",
            "DataSourceDefaultMaxConnections",
            "SourceQueryCulture",
            "MAttributes",
            "DiscourageCompositeModels",
            "AutomaticAggregationOptions",
            "DisableAutoExists",
            "MaxParallelismPerRefresh",
            "MaxParallelismPerQuery",
            "DirectLakeBehavior",
            "ValueFilterBehavior",
        ],
        vec![row],
    )
}

fn view_columns(model: &DataModel) -> TableValue {
    let ids = column_ids(model);
    let mut rows = Vec::new();
    for (table_index, table) in model.tables.iter().enumerate() {
        for (column_index, column) in table.columns.iter().enumerate() {
            rows.push(vec![
                number(ids[table_index][column_index]),
                text(&column.name),
                text(&table.name),
                optional(&column.data_type),
                blank(),
                blank(),
                flag(false),
                flag(false),
                flag(false),
                flag(true),
                blank(),
                text("Default"),
                blank(),
                text("Data"),
                text(&column.name),
                blank(),
                blank(),
                flag(true),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]);
        }
    }
    table(
        &[
            "ID",
            "Name",
            "Table",
            "DataType",
            "DataCategory",
            "Description",
            "IsHidden",
            "IsUnique",
            "IsKey",
            "IsNullable",
            "Alignment",
            "SummarizeBy",
            "ColumnStorage",
            "Type",
            "SourceColumn",
            "Expression",
            "FormatString",
            "IsAvailableInMDX",
            "SortByColumn",
            "GroupingBehavior",
            "SourceProviderType",
            "DisplayFolder",
            "AlternateOf",
            "LineageTag",
        ],
        rows,
    )
}

fn view_measures(model: &DataModel) -> TableValue {
    let rows = model
        .measures
        .iter()
        .enumerate()
        .map(|(index, measure)| {
            let home_table = measure.table.as_deref().map_or(Scalar::Blank, text);
            vec![
                number(index + 1),
                text(&measure.name),
                home_table,
                blank(),
                blank(),
                text(&measure.formula),
                optional(&measure.format),
                flag(false),
                text("Ready"),
                blank(),
                flag(false),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "Name",
            "Table",
            "Description",
            "DataType",
            "Expression",
            "FormatString",
            "IsHidden",
            "State",
            "KPIID",
            "IsSimpleMeasure",
            "DisplayFolder",
            "DetailRowsDefinition",
            "DataCategory",
            "FormatStringDefinition",
            "LineageTag",
        ],
        rows,
    )
}

fn view_relationships(model: &DataModel) -> TableValue {
    let rows = model
        .relationships
        .iter()
        .enumerate()
        .map(|(index, relationship)| {
            let name = relationship_name(relationship);
            vec![
                number(index + 1),
                text(&name),
                text(&name),
                text(MODEL_NAME),
                flag(!relationship.inactive),
                text(filtering_behavior_text(relationship.bidirectional)),
                flag(false),
                text(&relationship.from_table),
                text(&relationship.from_column),
                text("Many"),
                text(&relationship.to_table),
                text(&relationship.to_column),
                text("One"),
                text("Ready"),
                text(filtering_behavior_text(relationship.bidirectional)),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "Name",
            "Relationship",
            "Model",
            "IsActive",
            "CrossFilteringBehavior",
            "RelyOnReferentialIntegrity",
            "FromTable",
            "FromColumn",
            "FromCardinality",
            "ToTable",
            "ToColumn",
            "ToCardinality",
            "State",
            "SecurityFilteringBehavior",
        ],
        rows,
    )
}

fn view_tables(model: &DataModel) -> TableValue {
    let rows = model
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            vec![
                number(index + 1),
                text(&table.name),
                text(MODEL_NAME),
                blank(),
                blank(),
                flag(false),
                text("Import"),
                blank(),
                blank(),
                flag(false),
                flag(false),
                blank(),
                blank(),
            ]
        })
        .collect();
    table(
        &[
            "ID",
            "Name",
            "Model",
            "DataCategory",
            "Description",
            "IsHidden",
            "StorageMode",
            "TableStorage",
            "Expression",
            "ShowAsVariationOnly",
            "IsPrivate",
            "CalculationGroupPrecedence",
            "LineageTag",
        ],
        rows,
    )
}

pub(crate) fn call(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    None
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let upper = name.to_ascii_uppercase();
    // `INFO.STORAGETABLES` belongs to the `other` module; leave it (and non-INFO names) alone.
    if !upper.starts_with("INFO.") || upper == "INFO.STORAGETABLES" {
        return None;
    }
    // Restrictions arrive as name/value pairs. They are accepted but not applied; a dangling
    // name without a value is the one form the documented syntax rejects.
    if !args.len().is_multiple_of(2) {
        return Some(Err(EvalError::new("INFO function restrictions need a name and a value")));
    }
    let model = ev.model();
    let result = match upper.as_str() {
        "INFO.TABLES" => tables(model),
        "INFO.COLUMNS" => columns(model),
        "INFO.MEASURES" => measures(model),
        "INFO.RELATIONSHIPS" => relationships(model),
        "INFO.PARTITIONS" => partitions(model),
        "INFO.EXPRESSIONS" => expressions(model),
        "INFO.FUNCTIONS" => functions(),
        "INFO.MODEL" => model_table(),
        "INFO.VIEW.COLUMNS" => view_columns(model),
        "INFO.VIEW.MEASURES" => view_measures(model),
        "INFO.VIEW.RELATIONSHIPS" => view_relationships(model),
        "INFO.VIEW.TABLES" => view_tables(model),
        "INFO.ALTERNATEOFDEFINITIONS" => empty(&["ID", "ColumnID", "BaseColumnID", "BaseTableID", "Summarization"]),
        "INFO.ANNOTATIONS" => empty(&["ID", "ObjectID", "ObjectType", "Name", "Value", "ModifiedTime"]),
        "INFO.ATTRIBUTEHIERARCHIES" => empty(&["ID", "ColumnID", "State", "AttributeHierarchyStorageID", "ModifiedTime", "RefreshedTime"]),
        "INFO.ATTRIBUTEHIERARCHYSTORAGES" => empty(&[
            "ID",
            "AttributeHierarchyID",
            "SortOrder",
            "OptimizationLevel",
            "MaterializationType",
            "ColumnPositionToData",
            "ColumnDataToPosition",
            "DistinctDataCount",
            "DataVersion",
            "StorageFileID",
            "SystemTableID",
            "HasStatistics",
            "MinValue",
            "MaxValue",
            "StringValueMaxLength",
        ]),
        "INFO.CALCDEPENDENCY" => empty(&[
            "DATABASE_NAME",
            "OBJECT_TYPE",
            "TABLE",
            "OBJECT",
            "EXPRESSION",
            "REFERENCED_OBJECT_TYPE",
            "REFERENCED_TABLE",
            "REFERENCED_OBJECT",
            "REFERENCED_EXPRESSION",
            "QUERY",
        ]),
        "INFO.CALCULATIONGROUPS" => empty(&["ID", "TableID", "Description", "ModifiedTime", "Precedence"]),
        "INFO.CALCULATIONITEMS" => empty(&[
            "ID",
            "CalculationGroupID",
            "FormatStringDefinitionID",
            "Name",
            "Description",
            "ModifiedTime",
            "State",
            "ErrorMessage",
            "Expression",
            "Ordinal",
        ]),
        "INFO.CALENDARCOLUMNGROUPS" => empty(&["ID", "CalendarID", "TimeUnit", "ModifiedTime"]),
        "INFO.CALENDARCOLUMNREFERENCES" => empty(&["ID", "CalendarColumnGroupID", "ColumnID", "IsPrimaryColumn", "ModifiedTime"]),
        "INFO.CALENDARS" => empty(&["ID", "TableID", "Name", "Description", "LineageTag", "SourceLineageTag", "ModifiedTime"]),
        "INFO.CATALOGS" => empty(&[
            "CATALOG_NAME",
            "DESCRIPTION",
            "ROLES",
            "DATE_MODIFIED",
            "COMPATIBILITY_LEVEL",
            "TYPE",
            "VERSION",
            "DATABASE_ID",
            "DATABASE_GUID",
            "DATE_QUERIED",
            "CURRENTLY_USED",
            "POPULARITY",
            "WEIGHTEDPOPULARITY",
            "CLIENTCACHEREFRESHPOLICY",
            "ENCRYPTION_LEVEL",
        ]),
        "INFO.CHANGEDPROPERTIES" => empty(&["ID", "ObjectID", "ObjectType", "Property"]),
        "INFO.COLUMNPARTITIONSTORAGES" => {
            empty(&["ID", "ColumnStorageID", "PartitionStorageID", "DataVersion", "State", "SegmentStorageID", "StorageFileID"])
        }
        "INFO.COLUMNPERMISSIONS" => empty(&["ID", "TablePermissionID", "ColumnID", "ModifiedTime", "MetadataPermission"]),
        "INFO.COLUMNSTORAGES" => empty(&[
            "ID",
            "ColumnID",
            "Name",
            "StoragePosition",
            "DictionaryStorageID",
            "Settings",
            "ColumnFlags",
            "Collation",
            "OrderByColumn",
            "Locale",
            "BinaryCharacters",
            "Statistics_DistinctStates",
            "Statistics_MinDataID",
            "Statistics_MaxDataID",
            "Statistics_OriginalMinSegmentDataID",
            "Statistics_RLESortOrder",
            "Statistics_RowCount",
            "Statistics_HasNulls",
            "Statistics_RLERuns",
            "Statistics_OthersRLERuns",
            "Statistics_Usage",
            "Statistics_DBType",
            "Statistics_XMType",
            "Statistics_CompressionType",
            "Statistics_CompressionParam",
            "Statistics_EncodingHint",
            "IsDeltaPartitionColumn",
            "DeltaColumnMappingPhysicalName",
            "DeltaColumnMappingId",
            "FramedSourceColumn",
        ]),
        "INFO.CSDLMETADATA" => empty(&["METADATA"]),
        "INFO.CULTURES" => empty(&["ID", "ModelID", "Name", "LinguisticMetadataID", "ModifiedTime", "StructureModifiedTime"]),
        "INFO.DATACOVERAGEDEFINITIONS" => empty(&["ID", "PartitionID", "Description", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
        "INFO.DATASOURCES" => empty(&[
            "ID",
            "ModelID",
            "Name",
            "Description",
            "Type",
            "ConnectionString",
            "ImpersonationMode",
            "Account",
            "Password",
            "MaxConnections",
            "Isolation",
            "Timeout",
            "Provider",
            "ModifiedTime",
            "ConnectionDetails",
            "Options",
            "Credential",
            "ContextExpression",
        ]),
        "INFO.DELTATABLEMETADATASTORAGES" => empty(&[
            "ID",
            "PartitionStorageID",
            "TableName",
            "RootLocation",
            "CurrentVersion",
            "TableObjectID",
            "DatamartObjectID",
            "FramedSchemaName",
            "FallbackReason",
        ]),
        "INFO.DEPENDENCIES" => empty(&[
            "DATABASE_NAME",
            "OBJECT_TYPE",
            "TABLE",
            "OBJECT",
            "EXPRESSION",
            "REFERENCED_OBJECT_TYPE",
            "REFERENCED_TABLE",
            "REFERENCED_OBJECT",
            "REFERENCED_EXPRESSION",
            "QUERY",
        ]),
        "INFO.DETAILROWSDEFINITIONS" => empty(&["ID", "ObjectID", "ObjectType", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
        "INFO.DICTIONARYSTORAGES" => empty(&[
            "ID",
            "ColumnStorageID",
            "Type",
            "DataType",
            "DataVersion",
            "BaseId",
            "Magnitude",
            "LastId",
            "IsNullable",
            "IsUnique",
            "IsOperatingOn32",
            "DictionaryFlags",
            "StorageFileID",
            "Size",
        ]),
        "INFO.EXCLUDEDARTIFACTS" => empty(&["ID", "ObjectID", "ObjectType", "Reason"]),
        "INFO.EXTENDEDPROPERTIES" => empty(&["ID", "ObjectID", "ObjectType", "Name", "Type", "Value", "ModifiedTime"]),
        "INFO.FORMATSTRINGDEFINITIONS" => empty(&["ID", "ObjectID", "ObjectType", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
        "INFO.GENERALSEGMENTMAPSEGMENTMETADATASTORAGES" => empty(&["ID", "SegmentMapStorageID", "RecordCount", "Ordinal"]),
        "INFO.GROUPBYCOLUMNS" => empty(&["ID", "RelatedColumnDetailsID", "GroupingColumnID", "ModifiedTime"]),
        "INFO.HIERARCHIES" => empty(&[
            "ID",
            "TableID",
            "Name",
            "Description",
            "IsHidden",
            "State",
            "HierarchyStorageID",
            "ModifiedTime",
            "StructureModifiedTime",
            "RefreshedTime",
            "DisplayFolder",
            "HideMembers",
            "LineageTag",
            "SourceLineageTag",
        ]),
        "INFO.HIERARCHYSTORAGES" => empty(&["ID", "HierarchyID", "Name", "LevelDefinition", "MaterializationType", "StructureType", "SystemTableID"]),
        "INFO.KPIS" => empty(&[
            "ID",
            "MeasureID",
            "Description",
            "TargetDescription",
            "TargetExpression",
            "TargetFormatString",
            "StatusGraphic",
            "StatusDescription",
            "StatusExpression",
            "TrendGraphic",
            "TrendDescription",
            "TrendExpression",
            "ModifiedTime",
        ]),
        "INFO.LEVELS" => {
            empty(&["ID", "HierarchyID", "Ordinal", "Name", "Description", "ColumnID", "ModifiedTime", "LineageTag", "SourceLineageTag"])
        }
        "INFO.LINGUISTICMETADATA" => empty(&["ID", "CultureID", "Content", "ModifiedTime", "ContentType"]),
        "INFO.OBJECTTRANSLATIONS" => empty(&["ID", "CultureID", "ObjectID", "ObjectType", "Property", "Value", "ModifiedTime", "Altered"]),
        "INFO.PARQUETFILESTORAGES" => empty(&[
            "ID",
            "DeltaTableMetadataStorageID",
            "Size",
            "Location",
            "CreatedVersion",
            "DeletedVersion",
            "RowgroupCount",
            "Ordinal",
            "ETag",
            "DeletionVectorFileLocation",
            "DeletionVectorBitmapOffset",
            "DeletionVectorBitmapSize",
            "DeletionVectorFileETag",
        ]),
        "INFO.PARTITIONSTORAGES" => {
            empty(&["ID", "PartitionID", "Name", "StoragePosition", "SegmentMapStorageID", "DataObjectId", "StorageFolderID"])
        }
        "INFO.PERSPECTIVECOLUMNS" => empty(&["ID", "PerspectiveTableID", "ColumnID", "ModifiedTime"]),
        "INFO.PERSPECTIVEHIERARCHIES" => empty(&["ID", "PerspectiveTableID", "HierarchyID", "ModifiedTime"]),
        "INFO.PERSPECTIVEMEASURES" => empty(&["ID", "PerspectiveTableID", "MeasureID", "ModifiedTime"]),
        "INFO.PERSPECTIVES" => empty(&["ID", "ModelID", "Name", "Description", "ModifiedTime"]),
        "INFO.PERSPECTIVETABLES" => empty(&["ID", "PerspectiveID", "TableID", "IncludeAll", "ModifiedTime"]),
        "INFO.PROPERTIES" => empty(&["PropertyName", "PropertyDescription", "PropertyType", "PropertyAccessType", "IsRequired", "Value"]),
        "INFO.QUERYGROUPS" => empty(&["ID", "ModelID", "Folder", "Description"]),
        "INFO.REFRESHPOLICIES" => empty(&[
            "ID",
            "TableID",
            "PolicyType",
            "RollingWindowGranularity",
            "RollingWindowPeriods",
            "IncrementalGranularity",
            "IncrementalPeriods",
            "IncrementalPeriodsOffset",
            "PollingExpression",
            "SourceExpression",
            "Mode",
        ]),
        "INFO.RELATEDCOLUMNDETAILS" => empty(&["ID", "ColumnID", "ModifiedTime"]),
        "INFO.RELATIONSHIPINDEXSTORAGES" => empty(&[
            "ID",
            "RelationshipStorageID",
            "IndexType",
            "Flags",
            "RecordCount",
            "SecondaryRecordCount",
            "StorageFolderID",
            "StorageFileID",
            "SystemTableID",
            "SecondarySystemTableID",
        ]),
        "INFO.RELATIONSHIPSTORAGES" => {
            empty(&["ID", "RelationshipID", "Name", "DefinitionType", "Cardinality", "Flags", "RelationshipIndexStorageID"])
        }
        "INFO.ROLEMEMBERSHIPS" => empty(&["ID", "RoleID", "MemberName", "MemberID", "IdentityProvider", "MemberType", "ModifiedTime"]),
        "INFO.ROLES" => empty(&["ID", "ModelID", "Name", "Description", "ModelPermission", "ModifiedTime"]),
        "INFO.SEGMENTMAPSTORAGES" => empty(&["ID", "PartitionStorageID", "Type", "RecordCount", "SegmentCount", "RecordsPerSegment"]),
        "INFO.SEGMENTSTORAGES" => empty(&["ID", "ColumnPartitionStorageID", "SegmentCount", "StorageFileID"]),
        "INFO.STORAGEFILES" => empty(&["ID", "OwnerID", "OwnerType", "StorageFolderID", "FileName"]),
        "INFO.STORAGEFOLDERS" => empty(&["ID", "OwnerID", "OwnerType", "Path"]),
        "INFO.STORAGETABLECOLUMNS" => empty(&[
            "DATABASE_NAME",
            "CUBE_NAME",
            "MEASURE_GROUP_NAME",
            "DIMENSION_NAME",
            "ATTRIBUTE_NAME",
            "TABLE_ID",
            "COLUMN_ID",
            "COLUMN_TYPE",
            "COLUMN_ENCODING",
            "DATATYPE",
            "ISKEY",
            "ISUNIQUE",
            "ISNULLABLE",
            "ISROWNUMBER",
            "DICTIONARY_SIZE",
            "DICTIONARY_ISPAGEABLE",
            "DICTIONARY_ISRESIDENT",
            "DICTIONARY_TEMPERATURE",
            "DICTIONARY_LAST_ACCESSED",
        ]),
        "INFO.STORAGETABLECOLUMNSEGMENTS" => empty(&[
            "DATABASE_NAME",
            "CUBE_NAME",
            "MEASURE_GROUP_NAME",
            "PARTITION_NAME",
            "DIMENSION_NAME",
            "TABLE_ID",
            "COLUMN_ID",
            "SEGMENT_NUMBER",
            "TABLE_PARTITION_NUMBER",
            "RECORDS_COUNT",
            "ALLOCATED_SIZE",
            "USED_SIZE",
            "COMPRESSION_TYPE",
            "BITS_COUNT",
            "BOOKMARK_BITS_COUNT",
            "VERTIPAQ_STATE",
            "ISPAGEABLE",
            "ISRESIDENT",
            "TEMPERATURE",
            "LAST_ACCESSED",
        ]),
        "INFO.TABLEPERMISSIONS" => {
            empty(&["ID", "RoleID", "TableID", "FilterExpression", "ModifiedTime", "State", "ErrorMessage", "MetadataPermission"])
        }
        "INFO.TABLESTORAGES" => empty(&["ID", "TableID", "Name", "Version", "Settings", "RIViolationCount", "StorageFolderID"]),
        "INFO.USERDEFINEDFUNCTIONS" => empty(&[
            "ID",
            "ModelID",
            "Name",
            "Description",
            "Expression",
            "IsHidden",
            "State",
            "ErrorMessage",
            "ModifiedTime",
            "StructureModifiedTime",
            "LineageTag",
            "SourceLineageTag",
        ]),
        "INFO.VARIATIONS" => {
            empty(&["ID", "ColumnID", "Name", "Description", "RelationshipID", "DefaultHierarchyID", "DefaultColumnID", "IsDefault"])
        }
        _ => return None,
    };
    Some(Ok(result))
}
