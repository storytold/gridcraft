//! `INFO.*` metadata table functions: documented schemas and model-backed rows.

use gridcraft_dax::functions::catalog;
use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::String("East".into()), Scalar::Decimal(10.0)],
                    vec![Scalar::Integer(2), Scalar::String("West".into()), Scalar::Decimal(20.0)],
                    vec![Scalar::Integer(1), Scalar::String("East".into()), Scalar::Decimal(30.0)],
                ],
            },
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::Integer(1), Scalar::String("Widget".into())], vec![Scalar::Integer(2), Scalar::String("Gadget".into())]],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Product".into(),
            to_column: "ProductKey".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure {
                name: "Total Sales".into(),
                formula: "SUM(Sales[Amount])".into(),
                table: Some("Sales".into()),
                format: Some("#,0.00".into()),
                expr: None,
            },
            Measure { name: "Product Count".into(), formula: "COUNTROWS(Product)".into(), table: Some("Product".into()), format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn try_eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.message.clone())?;
    let expr = parse_expression(src).map_err(|error| error.message.clone())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.message.clone())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.message.clone())
}

fn eval_table(src: &str) -> TableValue {
    try_eval_table(src).unwrap_or_else(|error| panic!("{src}: {error}"))
}

fn column_names(table: &TableValue) -> Vec<String> {
    table.columns.iter().map(|column| column.name.clone()).collect()
}

fn cell(table: &TableValue, row: usize, column: &str) -> Scalar {
    let index = table.column_index(column).unwrap_or_else(|| panic!("missing column {column}"));
    table.rows[row][index].clone()
}

/// The documented column names of every `INFO.*` function in the assigned scope.
const SCHEMAS: &[(&str, &[&str])] = &[
    ("INFO.ALTERNATEOFDEFINITIONS", &["ID", "ColumnID", "BaseColumnID", "BaseTableID", "Summarization"]),
    ("INFO.ANNOTATIONS", &["ID", "ObjectID", "ObjectType", "Name", "Value", "ModifiedTime"]),
    ("INFO.ATTRIBUTEHIERARCHIES", &["ID", "ColumnID", "State", "AttributeHierarchyStorageID", "ModifiedTime", "RefreshedTime"]),
    (
        "INFO.ATTRIBUTEHIERARCHYSTORAGES",
        &[
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
        ],
    ),
    (
        "INFO.CALCDEPENDENCY",
        &[
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
        ],
    ),
    ("INFO.CALCULATIONGROUPS", &["ID", "TableID", "Description", "ModifiedTime", "Precedence"]),
    (
        "INFO.CALCULATIONITEMS",
        &[
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
        ],
    ),
    ("INFO.CALENDARCOLUMNGROUPS", &["ID", "CalendarID", "TimeUnit", "ModifiedTime"]),
    ("INFO.CALENDARCOLUMNREFERENCES", &["ID", "CalendarColumnGroupID", "ColumnID", "IsPrimaryColumn", "ModifiedTime"]),
    ("INFO.CALENDARS", &["ID", "TableID", "Name", "Description", "LineageTag", "SourceLineageTag", "ModifiedTime"]),
    (
        "INFO.CATALOGS",
        &[
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
        ],
    ),
    ("INFO.CHANGEDPROPERTIES", &["ID", "ObjectID", "ObjectType", "Property"]),
    ("INFO.COLUMNPARTITIONSTORAGES", &["ID", "ColumnStorageID", "PartitionStorageID", "DataVersion", "State", "SegmentStorageID", "StorageFileID"]),
    ("INFO.COLUMNPERMISSIONS", &["ID", "TablePermissionID", "ColumnID", "ModifiedTime", "MetadataPermission"]),
    (
        "INFO.COLUMNS",
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
    ),
    (
        "INFO.COLUMNSTORAGES",
        &[
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
        ],
    ),
    ("INFO.CSDLMETADATA", &["METADATA"]),
    ("INFO.CULTURES", &["ID", "ModelID", "Name", "LinguisticMetadataID", "ModifiedTime", "StructureModifiedTime"]),
    ("INFO.DATACOVERAGEDEFINITIONS", &["ID", "PartitionID", "Description", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
    (
        "INFO.DATASOURCES",
        &[
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
        ],
    ),
    (
        "INFO.DELTATABLEMETADATASTORAGES",
        &[
            "ID",
            "PartitionStorageID",
            "TableName",
            "RootLocation",
            "CurrentVersion",
            "TableObjectID",
            "DatamartObjectID",
            "FramedSchemaName",
            "FallbackReason",
        ],
    ),
    (
        "INFO.DEPENDENCIES",
        &[
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
        ],
    ),
    ("INFO.DETAILROWSDEFINITIONS", &["ID", "ObjectID", "ObjectType", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
    (
        "INFO.DICTIONARYSTORAGES",
        &[
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
        ],
    ),
    ("INFO.EXCLUDEDARTIFACTS", &["ID", "ObjectID", "ObjectType", "Reason"]),
    (
        "INFO.EXPRESSIONS",
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
    ),
    ("INFO.EXTENDEDPROPERTIES", &["ID", "ObjectID", "ObjectType", "Name", "Type", "Value", "ModifiedTime"]),
    ("INFO.FORMATSTRINGDEFINITIONS", &["ID", "ObjectID", "ObjectType", "Expression", "ModifiedTime", "State", "ErrorMessage"]),
    (
        "INFO.FUNCTIONS",
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
    ),
    ("INFO.GENERALSEGMENTMAPSEGMENTMETADATASTORAGES", &["ID", "SegmentMapStorageID", "RecordCount", "Ordinal"]),
    ("INFO.GROUPBYCOLUMNS", &["ID", "RelatedColumnDetailsID", "GroupingColumnID", "ModifiedTime"]),
    (
        "INFO.HIERARCHIES",
        &[
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
        ],
    ),
    ("INFO.HIERARCHYSTORAGES", &["ID", "HierarchyID", "Name", "LevelDefinition", "MaterializationType", "StructureType", "SystemTableID"]),
    (
        "INFO.KPIS",
        &[
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
        ],
    ),
    ("INFO.LEVELS", &["ID", "HierarchyID", "Ordinal", "Name", "Description", "ColumnID", "ModifiedTime", "LineageTag", "SourceLineageTag"]),
    ("INFO.LINGUISTICMETADATA", &["ID", "CultureID", "Content", "ModifiedTime", "ContentType"]),
    (
        "INFO.MEASURES",
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
    ),
    (
        "INFO.MODEL",
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
    ),
    ("INFO.OBJECTTRANSLATIONS", &["ID", "CultureID", "ObjectID", "ObjectType", "Property", "Value", "ModifiedTime", "Altered"]),
    (
        "INFO.PARQUETFILESTORAGES",
        &[
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
        ],
    ),
    (
        "INFO.PARTITIONS",
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
    ),
    ("INFO.PARTITIONSTORAGES", &["ID", "PartitionID", "Name", "StoragePosition", "SegmentMapStorageID", "DataObjectId", "StorageFolderID"]),
    ("INFO.PERSPECTIVECOLUMNS", &["ID", "PerspectiveTableID", "ColumnID", "ModifiedTime"]),
    ("INFO.PERSPECTIVEHIERARCHIES", &["ID", "PerspectiveTableID", "HierarchyID", "ModifiedTime"]),
    ("INFO.PERSPECTIVEMEASURES", &["ID", "PerspectiveTableID", "MeasureID", "ModifiedTime"]),
    ("INFO.PERSPECTIVES", &["ID", "ModelID", "Name", "Description", "ModifiedTime"]),
    ("INFO.PERSPECTIVETABLES", &["ID", "PerspectiveID", "TableID", "IncludeAll", "ModifiedTime"]),
    ("INFO.PROPERTIES", &["PropertyName", "PropertyDescription", "PropertyType", "PropertyAccessType", "IsRequired", "Value"]),
    ("INFO.QUERYGROUPS", &["ID", "ModelID", "Folder", "Description"]),
    (
        "INFO.REFRESHPOLICIES",
        &[
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
        ],
    ),
    ("INFO.RELATEDCOLUMNDETAILS", &["ID", "ColumnID", "ModifiedTime"]),
    (
        "INFO.RELATIONSHIPINDEXSTORAGES",
        &[
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
        ],
    ),
    (
        "INFO.RELATIONSHIPS",
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
    ),
    ("INFO.RELATIONSHIPSTORAGES", &["ID", "RelationshipID", "Name", "DefinitionType", "Cardinality", "Flags", "RelationshipIndexStorageID"]),
    ("INFO.ROLEMEMBERSHIPS", &["ID", "RoleID", "MemberName", "MemberID", "IdentityProvider", "MemberType", "ModifiedTime"]),
    ("INFO.ROLES", &["ID", "ModelID", "Name", "Description", "ModelPermission", "ModifiedTime"]),
    ("INFO.SEGMENTMAPSTORAGES", &["ID", "PartitionStorageID", "Type", "RecordCount", "SegmentCount", "RecordsPerSegment"]),
    ("INFO.SEGMENTSTORAGES", &["ID", "ColumnPartitionStorageID", "SegmentCount", "StorageFileID"]),
    ("INFO.STORAGEFILES", &["ID", "OwnerID", "OwnerType", "StorageFolderID", "FileName"]),
    ("INFO.STORAGEFOLDERS", &["ID", "OwnerID", "OwnerType", "Path"]),
    (
        "INFO.STORAGETABLECOLUMNS",
        &[
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
        ],
    ),
    (
        "INFO.STORAGETABLECOLUMNSEGMENTS",
        &[
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
        ],
    ),
    ("INFO.TABLEPERMISSIONS", &["ID", "RoleID", "TableID", "FilterExpression", "ModifiedTime", "State", "ErrorMessage", "MetadataPermission"]),
    (
        "INFO.TABLES",
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
    ),
    ("INFO.TABLESTORAGES", &["ID", "TableID", "Name", "Version", "Settings", "RIViolationCount", "StorageFolderID"]),
    (
        "INFO.USERDEFINEDFUNCTIONS",
        &[
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
        ],
    ),
    ("INFO.VARIATIONS", &["ID", "ColumnID", "Name", "Description", "RelationshipID", "DefaultHierarchyID", "DefaultColumnID", "IsDefault"]),
    (
        "INFO.VIEW.COLUMNS",
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
    ),
    (
        "INFO.VIEW.MEASURES",
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
    ),
    (
        "INFO.VIEW.RELATIONSHIPS",
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
    ),
    (
        "INFO.VIEW.TABLES",
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
    ),
];

#[test]
fn every_info_function_returns_its_documented_schema() {
    assert_eq!(SCHEMAS.len(), 71, "assigned INFO.* functions");
    for (name, columns) in SCHEMAS {
        let table = eval_table(&format!("{name}()"));
        let expected: Vec<String> = columns.iter().map(|column| (*column).to_string()).collect();
        assert_eq!(column_names(&table), expected, "{name} columns");
        for row in &table.rows {
            assert_eq!(row.len(), table.columns.len(), "{name} row width");
        }
    }
}

#[test]
fn tables_describe_the_model_tables() {
    let table = eval_table("INFO.TABLES()");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(cell(&table, 0, "ID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "ModelID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Sales".into()));
    assert_eq!(cell(&table, 0, "IsHidden"), Scalar::Boolean(false));
    assert_eq!(cell(&table, 1, "ID"), Scalar::Integer(2));
    assert_eq!(cell(&table, 1, "Name"), Scalar::String("Product".into()));
}

#[test]
fn columns_describe_the_model_columns() {
    let table = eval_table("INFO.COLUMNS()");
    assert_eq!(table.rows.len(), 5);
    let names: Vec<Scalar> = (0..5).map(|row| cell(&table, row, "ExplicitName")).collect();
    assert_eq!(
        names,
        vec![
            Scalar::String("ProductKey".into()),
            Scalar::String("Region".into()),
            Scalar::String("Amount".into()),
            Scalar::String("ProductKey".into()),
            Scalar::String("Name".into()),
        ]
    );
    let table_ids: Vec<Scalar> = (0..5).map(|row| cell(&table, row, "TableID")).collect();
    assert_eq!(table_ids, vec![Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(2), Scalar::Integer(2)]);
    let ids: Vec<Scalar> = (0..5).map(|row| cell(&table, row, "ID")).collect();
    assert_eq!(ids, vec![Scalar::Integer(1), Scalar::Integer(2), Scalar::Integer(3), Scalar::Integer(4), Scalar::Integer(5)]);
    assert_eq!(cell(&table, 2, "ExplicitDataType"), Scalar::String("Double".into()));
    assert_eq!(cell(&table, 2, "DisplayOrdinal"), Scalar::Integer(3));
}

#[test]
fn measures_expose_name_formula_and_home_table() {
    let table = eval_table("INFO.MEASURES()");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Total Sales".into()));
    assert_eq!(cell(&table, 0, "Expression"), Scalar::String("SUM(Sales[Amount])".into()));
    assert_eq!(cell(&table, 0, "TableID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "FormatString"), Scalar::String("#,0.00".into()));
    assert_eq!(cell(&table, 0, "State"), Scalar::Integer(1));
    assert_eq!(cell(&table, 1, "Name"), Scalar::String("Product Count".into()));
    assert_eq!(cell(&table, 1, "TableID"), Scalar::Integer(2));
    assert_eq!(cell(&table, 1, "FormatString"), Scalar::Blank);
}

#[test]
fn relationships_join_columns_by_id() {
    let table = eval_table("INFO.RELATIONSHIPS()");
    assert_eq!(table.rows.len(), 1);
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Sales[ProductKey] -> Product[ProductKey]".into()));
    assert_eq!(cell(&table, 0, "IsActive"), Scalar::Boolean(true));
    assert_eq!(cell(&table, 0, "FromTableID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "FromColumnID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "FromCardinality"), Scalar::Integer(2));
    assert_eq!(cell(&table, 0, "ToTableID"), Scalar::Integer(2));
    assert_eq!(cell(&table, 0, "ToColumnID"), Scalar::Integer(4));
    assert_eq!(cell(&table, 0, "ToCardinality"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "CrossFilteringBehavior"), Scalar::Integer(1));
}

#[test]
fn partitions_cover_each_table() {
    let table = eval_table("INFO.PARTITIONS()");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(cell(&table, 0, "TableID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Sales".into()));
    assert_eq!(cell(&table, 0, "Mode"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "State"), Scalar::Integer(1));
    assert_eq!(cell(&table, 1, "TableID"), Scalar::Integer(2));
    assert_eq!(cell(&table, 1, "Name"), Scalar::String("Product".into()));
}

#[test]
fn expressions_list_measure_formulas() {
    let table = eval_table("INFO.EXPRESSIONS()");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(cell(&table, 0, "ModelID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Total Sales".into()));
    assert_eq!(cell(&table, 0, "Kind"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "Expression"), Scalar::String("SUM(Sales[Amount])".into()));
    assert_eq!(cell(&table, 1, "Expression"), Scalar::String("COUNTROWS(Product)".into()));
}

#[test]
fn functions_match_the_catalog() {
    let table = eval_table("INFO.FUNCTIONS()");
    assert_eq!(table.rows.len(), catalog().len());
    let sum = (0..table.rows.len()).find(|row| cell(&table, *row, "FUNCTION_NAME") == Scalar::String("SUM".into())).expect("SUM row");
    assert_eq!(cell(&table, sum, "RETURN_TYPE"), Scalar::String("Numeric".into()));
    assert_eq!(cell(&table, sum, "ORIGIN"), Scalar::Integer(1));
    assert!(!cell(&table, sum, "DESCRIPTION").to_text().is_empty());
    let info_tables =
        (0..table.rows.len()).find(|row| cell(&table, *row, "FUNCTION_NAME") == Scalar::String("INFO.TABLES".into())).expect("INFO.TABLES row");
    assert_eq!(cell(&table, info_tables, "RETURN_TYPE"), Scalar::String("Table".into()));
}

#[test]
fn model_has_one_synthetic_row() {
    let table = eval_table("INFO.MODEL()");
    assert_eq!(table.rows.len(), 1);
    assert_eq!(cell(&table, 0, "ID"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "Name"), Scalar::String("Model".into()));
    assert_eq!(cell(&table, 0, "Version"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "DefaultMode"), Scalar::Integer(1));
    assert_eq!(cell(&table, 0, "DiscourageImplicitMeasures"), Scalar::Boolean(true));
}

#[test]
fn view_functions_decode_ids_to_names() {
    let columns = eval_table("INFO.VIEW.COLUMNS()");
    assert_eq!(columns.rows.len(), 5);
    assert_eq!(cell(&columns, 2, "Name"), Scalar::String("Amount".into()));
    assert_eq!(cell(&columns, 2, "Table"), Scalar::String("Sales".into()));
    assert_eq!(cell(&columns, 2, "DataType"), Scalar::String("Double".into()));
    assert_eq!(cell(&columns, 2, "Type"), Scalar::String("Data".into()));
    assert_eq!(cell(&columns, 2, "SourceColumn"), Scalar::String("Amount".into()));
    assert_eq!(cell(&columns, 2, "IsAvailableInMDX"), Scalar::Boolean(true));

    let measures = eval_table("INFO.VIEW.MEASURES()");
    assert_eq!(measures.rows.len(), 2);
    assert_eq!(cell(&measures, 0, "Name"), Scalar::String("Total Sales".into()));
    assert_eq!(cell(&measures, 0, "Table"), Scalar::String("Sales".into()));
    assert_eq!(cell(&measures, 0, "Expression"), Scalar::String("SUM(Sales[Amount])".into()));
    assert_eq!(cell(&measures, 0, "State"), Scalar::String("Ready".into()));

    let relationships = eval_table("INFO.VIEW.RELATIONSHIPS()");
    assert_eq!(relationships.rows.len(), 1);
    assert_eq!(cell(&relationships, 0, "Model"), Scalar::String("Model".into()));
    assert_eq!(cell(&relationships, 0, "FromTable"), Scalar::String("Sales".into()));
    assert_eq!(cell(&relationships, 0, "FromColumn"), Scalar::String("ProductKey".into()));
    assert_eq!(cell(&relationships, 0, "FromCardinality"), Scalar::String("Many".into()));
    assert_eq!(cell(&relationships, 0, "ToTable"), Scalar::String("Product".into()));
    assert_eq!(cell(&relationships, 0, "ToColumn"), Scalar::String("ProductKey".into()));
    assert_eq!(cell(&relationships, 0, "ToCardinality"), Scalar::String("One".into()));
    assert_eq!(cell(&relationships, 0, "CrossFilteringBehavior"), Scalar::String("OneDirection".into()));

    let tables = eval_table("INFO.VIEW.TABLES()");
    assert_eq!(tables.rows.len(), 2);
    assert_eq!(cell(&tables, 0, "Name"), Scalar::String("Sales".into()));
    assert_eq!(cell(&tables, 0, "Model"), Scalar::String("Model".into()));
    assert_eq!(cell(&tables, 0, "StorageMode"), Scalar::String("Import".into()));
}

#[test]
fn unrepresentable_objects_keep_their_columns_without_rows() {
    for name in [
        "INFO.DATASOURCES",
        "INFO.CALCDEPENDENCY",
        "INFO.CULTURES",
        "INFO.PERSPECTIVES",
        "INFO.ROLES",
        "INFO.TABLEPERMISSIONS",
        "INFO.QUERYGROUPS",
        "INFO.STORAGEFILES",
    ] {
        let table = eval_table(&format!("{name}()"));
        assert!(!table.columns.is_empty(), "{name} has documented columns");
        assert!(table.rows.is_empty(), "{name} has no model-backed rows");
    }
}

#[test]
fn restrictions_are_accepted_as_pairs() {
    let table = eval_table("INFO.TABLES(\"Name\", \"Sales\")");
    assert_eq!(table.rows.len(), 2);
    let error = try_eval_table("INFO.TABLES(\"Name\")").expect_err("dangling restriction");
    assert!(error.contains("restriction"), "unexpected error: {error}");
}

#[test]
fn non_info_calls_are_left_to_other_modules() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("SUM(Sales[Amount])").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound), Ok(Scalar::Decimal(60.0)));
}
