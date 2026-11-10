//! The public data model returned by [`crate::model_file::read_model`] and accepted by
//! [`crate::model_file::write_model`].
//!
//! The shapes mirror the VertiPaq storage contract: tables in cube dimension order, columns in
//! their `Columns` collection order (the internal row-number column is never surfaced), and one
//! [`Scalar`] per row with [`Scalar::Blank`] for nulls. Values are in storage order, which is
//! stable across a read/write round trip.

use serde::{Deserialize, Serialize};

use crate::value::Scalar;

/// A complete spreadsheet data model: tables, relationships and user measures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelData {
    /// Tables in the cube's `<Dimensions>` order.
    #[serde(default)]
    pub tables: Vec<ModelTableData>,
    /// Relationships; Excel writes many-to-one only.
    #[serde(default)]
    pub relationships: Vec<ModelRelationship>,
    /// User measures, in MDX script order.
    #[serde(default)]
    pub measures: Vec<super::ModelMeasure>,
}

/// One table (cube dimension) and its columns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelTableData {
    /// Display name: the cube `<Dimension><Name>`, not the internal dimension ID.
    pub name: String,
    /// `false` for hidden dimensions such as the internal `Metrics` table.
    #[serde(default = "default_visible")]
    pub visible: bool,
    /// Columns in `Columns` collection order, without the internal row-number column.
    #[serde(default)]
    pub columns: Vec<ModelColumnData>,
}

fn default_visible() -> bool {
    true
}

/// One stored column and its rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelColumnData {
    /// Display name when the dimension metadata carries one, otherwise the storage name.
    pub name: String,
    /// Declared type in the engine's vocabulary: `Int64`, `Double`, `String`, `DateTime`,
    /// `Boolean`, `Currency`, or `Empty` for a column that stores nothing.
    pub data_type: String,
    /// One entry per row. `Scalar::Blank` is the explicit null representation.
    #[serde(default)]
    pub values: Vec<Scalar>,
}

/// Relationship cardinality as recorded by the dimension metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationshipMultiplicity {
    /// Many rows on the foreign side match one row on the primary side (the Excel default).
    #[default]
    ManyToOne,
    /// One row on the foreign side matches many on the primary side.
    OneToMany,
    /// One row matches one row.
    OneToOne,
    /// The metadata's multiplicity pair is not one of the above.
    Unknown,
}

/// One relationship: the many (foreign) end points at the one (primary) end.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRelationship {
    pub foreign_table: String,
    pub foreign_column: String,
    pub primary_table: String,
    pub primary_column: String,
    /// The relationship is active (metadata `<Visible>true</Visible>`).
    #[serde(default = "default_visible")]
    pub active: bool,
    #[serde(default)]
    pub multiplicity: RelationshipMultiplicity,
}
