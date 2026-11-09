//! Wire types of the collaboration protocol (`docs/collab/rfc.md` §4.2).

use serde::{Deserialize, Serialize};

/// The form of an operation (ADR-0001). The conflict class is decided by the command's content
/// (ADR-0004), not by this value, but a `structural` operation is always in the structural class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Command,
    Structural,
}

/// Where an operation applies. `sheet` is a session-scoped [`SheetKeys`](crate::SheetKeys) key as
/// text; `a1` is the target address or range the UI selected when the command was made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub sheet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a1: Option<String>,
}

/// The command itself: an id of the engine registry and its parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub cmd: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// One participant's operation: the whole envelope of ADR-0001.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientOp {
    pub op_id: String,
    pub doc: String,
    #[serde(default)]
    pub base_version: u64,
    #[serde(default)]
    pub epoch: u64,
    pub kind: Kind,
    pub target: Target,
    pub payload: Payload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// The command as the server applied it, so a client can echo it (and run the same code path for
/// effects the delta does not describe, such as a chart's geometry).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEcho {
    pub id: String,
    pub params: serde_json::Value,
}

/// One cell of an authoritative delta. `style` is the interned style id of the workbook the delta
/// comes from; clients that share the same base workbook resolve it identically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellChange {
    pub a1: String,
    #[serde(default)]
    pub value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<u32>,
}

/// The changed cells of one sheet, plus the objects the command removed or created.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetDelta {
    pub sheet: String,
    pub cells: Vec<CellChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects_deleted: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects_created: Vec<u32>,
}

/// What the server did: the command it applied and the cells that changed. At least one is
/// present; cells-only operations carry both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandEcho>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Vec<SheetDelta>>,
}

/// One sheet of a session: its key on the wire and its name. Sent in `welcome`, and again with
/// every structural `applied` because a rename, insert or delete changes the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetName {
    pub key: String,
    pub name: String,
}

/// A whole sheet as it stood at one boundary of a structural step (ADR-0010 §Structural undo).
///
/// A row insert or delete moves every address below it, and a sheet insert or delete changes the
/// set of sheets, so the cells a structural step touched are not a delta: an undo needs the sheet
/// as it was, not a list of changed cells. The room keeps such an image before and after the step
/// and sends it with the undo/redo so every replica replaces the same sheets in one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetImage {
    /// The session key of the sheet. It survives the undo, so presence and later operations keep
    /// addressing the same sheet.
    pub key: String,
    pub name: String,
    /// Where the sheet sat in the workbook at that boundary.
    pub index: u32,
    /// The sheet itself: cells, formulas, styles, merges, tables, charts and its view.
    #[serde(with = "sheet_value")]
    pub sheet: gridcraft_model::Sheet,
}

/// One part of a serialised sheet image that does not fit a single frame (ADR-0013).
///
/// The image of a structural undo is the UTF-8 JSON of the operation's `images`; `data` is a
/// base64 slice of it. `seq` counts from 0 and `total` is fixed for the whole operation, so a
/// client that misses a part in the middle refuses the image instead of applying a prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePart {
    pub seq: u32,
    pub total: u32,
    /// Base64 of a byte slice of the operation's serialised `images`.
    pub data: String,
}

/// Serde for a [`gridcraft_model::Sheet`] inside an internally tagged [`crate::ServerMessage`].
///
/// The tag makes serde buffer a message's fields as content, and content keeps JSON object keys as
/// strings: a sheet's row, column and band maps are keyed by numbers, so they would refuse to read
/// back. Round-tripping the sheet through `serde_json` gives its inner maps the ordinary JSON
/// deserializer, which turns a string key into the number the map holds.
mod sheet_value {
    use gridcraft_model::Sheet;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

    pub fn serialize<S: Serializer>(sheet: &Sheet, serializer: S) -> Result<S::Ok, S::Error> {
        serde_json::to_value(sheet).map_err(serde::ser::Error::custom)?.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Sheet, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        serde_json::from_value(value).map_err(D::Error::custom)
    }
}

/// A confirmed operation: the only thing a client applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub op_id: String,
    pub version: u64,
    pub epoch: u64,
    pub actor: String,
    pub result: Outcome,
    /// The sheet map after a structural operation: keys never change, names and order may.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheets: Option<Vec<SheetName>>,
    /// Whole-sheet images of a structural undo/redo: the sheets as they stood before the step
    /// (undo) or after it (redo). The client replaces its sheets with them, then applies `sheets`;
    /// such an answer has no command echo and no cell delta, because every address may have moved.
    /// An image over one frame travels as `imagePart` parts instead (ADR-0013).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<SheetImage>>,
    /// True while more `applied` frames of the same operation follow (ADR-0013). `version`,
    /// `epoch` and `opId` repeat on every part, the parts of one operation go out back to back,
    /// and a client applies the operation only after the part with `more:false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub more: bool,
    /// One part of the serialised `images` when the image did not fit a single frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_part: Option<ImagePart>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Where the client should resynchronise after a rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub version: u64,
    #[serde(default)]
    pub epoch: u64,
}

/// Why an operation was refused (ADR-0004).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    /// The command is not in the allowlist, or the role cannot write.
    Forbidden,
    /// The parameters are wrong for the command.
    Invalid,
    /// The address moved (a structural edit) or the object is gone; resync.
    Stale,
    /// The engine refused the edit itself (sheet protection, part of an array, …).
    Conflict,
    /// The operation or its result is over a size limit.
    TooLarge,
    /// The server could not run the command.
    Internal,
}

impl RejectCode {
    /// The wire code, as `serde` writes it; the same string labels the metric.
    pub fn code(self) -> &'static str {
        match self {
            RejectCode::Forbidden => "forbidden",
            RejectCode::Invalid => "invalid",
            RejectCode::Stale => "stale",
            RejectCode::Conflict => "conflict",
            RejectCode::TooLarge => "too_large",
            RejectCode::Internal => "internal",
        }
    }
}

/// A refusal. `retryable` tells the client whether sending the same operation again with the new
/// version can succeed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub op_id: String,
    pub code: RejectCode,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
