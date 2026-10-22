//! Relationship management functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The four functions mirror <https://dax.guide/functions/relationships-management/> in index
//! order. `CROSSFILTER` and `USERELATIONSHIP` are filter modifiers: they return no value and
//! only alter filter propagation while the filter context that contains them is evaluated.
//! `RELATED` and `RELATEDTABLE` traverse an existing relationship from a row context.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, Overload, ParamKind, Parameter, ReturnType, overload, required, spec, standard};

/// Signature of `CROSSFILTER`, which names both endpoint columns and a direction.
const CROSSFILTER_PARAMS: &[Parameter] =
    &[required("LeftColumnName", ParamKind::Column), required("RightColumnName", ParamKind::Column), required("CrossFilterType", ParamKind::Any)];

const CROSSFILTER_OVERLOADS: &[Overload] = &[overload(&[ParamKind::Column, ParamKind::Column, ParamKind::Any])];

const USERELATIONSHIP_PARAMS: &[Parameter] = &[required("ColumnName1", ParamKind::Column), required("ColumnName2", ParamKind::Column)];

const USERELATIONSHIP_OVERLOADS: &[Overload] = &[overload(&[ParamKind::Column, ParamKind::Column])];

/// Flags shared by the relationship management functions: none of them is an iterator or
/// volatile, dax.guide recommends all of them, and every one is usable in Excel and restricted
/// by DirectQuery exactly like the rest of the catalog.
const RELATIONSHIP_FLAGS: FunctionFlags = standard(false);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "CROSSFILTER",
        FunctionCategory::RelationshipManagement,
        3,
        3,
        CROSSFILTER_PARAMS,
        CROSSFILTER_OVERLOADS,
        ReturnType::Any,
        RELATIONSHIP_FLAGS,
        "Overrides the cross-filter direction of an existing relationship for the duration of \
         the filter context it modifies. The two column arguments identify the relationship \
         endpoints; the third argument is 0/1/2/3/4 or the words NONE, ONEWAY, BOTH, \
         ONEWAY_RIGHTFILTERSLEFT and ONEWAY_LEFTFILTERSRIGHT. It changes filter propagation in \
         both directions, never which relationship is active, so it is used as a filter \
         modifier inside CALCULATE and similar functions and returns no value.",
        &[
            "CALCULATE ( [Sales Amount], CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], BOTH ) )",
            "CALCULATE ( COUNTROWS ( Customer ), CROSSFILTER ( Sales[ProductKey], 'Product'[ProductKey], NONE ) )",
            "CALCULATE ( [Sales Amount], CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], 1 ) )",
        ],
    ),
    spec(
        "RELATED",
        FunctionCategory::RelationshipManagement,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Any,
        RELATIONSHIP_FLAGS,
        "Returns the value of a column on the one side of a many-to-one relationship, following \
         one or more regular relationships from the current row. It requires a row context and \
         an active regular relationship, so it is used in calculated columns and inside table \
         iterators; a limited relationship cannot be traversed. The result is BLANK when no \
         related row exists and the column value can be of any type.",
        &[
            "SUMX ( Sales, Sales[Quantity] * RELATED ( 'Product'[List Price] ) )",
            "RELATED ( 'Product'[Category] )",
            "RELATED ( Customer[CountryRegion] )",
        ],
    ),
    spec(
        "RELATEDTABLE",
        FunctionCategory::RelationshipManagement,
        1,
        1,
        &[required("Table", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        RELATIONSHIP_FLAGS,
        "Returns the rows of a table on the many side of a relationship, filtered to the rows \
         related to the current row. It performs a context transition from the row context to a \
         filter context before evaluating the table, making it a shortcut for CALCULATETABLE \
         with no additional filters; unlike CALCULATETABLE it accepts only a table reference, \
         not an arbitrary table expression.",
        &["AVERAGEX ( Customer, COUNTROWS ( RELATEDTABLE ( Sales ) ) )", "RELATEDTABLE ( Sales )", "COUNTROWS ( RELATEDTABLE ( InternetSales ) )"],
    ),
    spec(
        "USERELATIONSHIP",
        FunctionCategory::RelationshipManagement,
        2,
        2,
        USERELATIONSHIP_PARAMS,
        USERELATIONSHIP_OVERLOADS,
        ReturnType::Any,
        RELATIONSHIP_FLAGS,
        "Activates an existing relationship for the duration of the filter context it modifies, \
         so its filters propagate even when the relationship is inactive in the model. The two \
         column arguments are the relationship endpoints; the innermost USERELATIONSHIP wins in \
         nested CALCULATE expressions. It selects which relationship propagates filters, not \
         their cross-filter direction, and returns no value.",
        &[
            "CALCULATE ( [Sales Amount], USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] ) )",
            "CALCULATE ( COUNTROWS ( Sales ), USERELATIONSHIP ( Sales[Order Date], 'Date'[Date] ) )",
        ],
    ),
];
