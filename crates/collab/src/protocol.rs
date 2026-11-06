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

/// A confirmed operation: the only thing a client applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub op_id: String,
    pub version: u64,
    pub epoch: u64,
    pub actor: String,
    pub result: Outcome,
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
