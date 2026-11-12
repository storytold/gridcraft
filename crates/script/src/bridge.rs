//! The ExcelScript bridge: one JSON object per line over the runtime's stdin and stdout.
//!
//! The runtime's shim sends a request for every workbook operation it needs
//! ([`Request`]); GridCraft answers with a [`Response`] carrying the result or a
//! [`BridgeError`]. Both sides are bounded: a line longer than [`MAX_LINE_BYTES`] is refused
//! before it is parsed, and an unknown method is an error rather than a guess.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Longest line either side accepts, in bytes. A `range.setValues` call can carry a large block,
/// so this is generous; it still bounds what one message can make the other side allocate.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
/// Longest method name accepted.
pub const MAX_METHOD_LEN: usize = 120;

/// One workbook operation the runtime asks for, independent of the wire envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BridgeCall {
    pub method: String,
    #[serde(default)]
    pub params: Json,
}

impl BridgeCall {
    pub fn new(method: impl Into<String>, params: Json) -> Self {
        BridgeCall { method: method.into(), params }
    }

    /// The worksheet the call names, when it names one (`"Sheet1!A1"` is not a `sheet`; the
    /// recorder writes one or the other).
    pub fn sheet(&self) -> Option<&str> {
        self.params.get("sheet").and_then(Json::as_str)
    }

    pub fn range(&self) -> Option<&str> {
        self.params.get("range").and_then(Json::as_str)
    }
}

/// A request from the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Json,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Json) -> Self {
        Request { id, method: method.into(), params }
    }

    pub fn call(&self) -> BridgeCall {
        BridgeCall::new(self.method.clone(), self.params.clone())
    }
}

/// A reply to a [`Request`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

impl Response {
    pub fn ok(id: u64, result: Json) -> Self {
        Response { id, result: Some(result), error: None }
    }

    pub fn failed(id: u64, error: BridgeError) -> Self {
        Response { id, result: None, error: Some(error) }
    }

    /// The reply as one line, without its newline. A reply always ends up serializable: its
    /// fields are a number, JSON values parsed from a line and strings.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| format!("{{\"id\":{},\"error\":{{\"code\":\"protocol\",\"message\":\"the reply could not be written\"}}}}", self.id))
    }
}

/// What went wrong with one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The line was not a request, or named something outside the protocol.
    BadRequest,
    /// The operation's parameters did not make sense (a bad address, a value of the wrong type).
    InvalidArgument,
    /// A GridCraft command refused it (the workbook is protected, the sheet is gone, …).
    Engine,
    /// A limit was reached (calls, output, time).
    Limit,
    /// The member is in the ExcelScript reference but GridCraft cannot back it (no model, or not
    /// implemented yet). The message names the member.
    NotSupported,
    /// The runtime failed outside a call.
    Runtime,
    /// The other side broke the protocol.
    Protocol,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::BadRequest => "badRequest",
            ErrorCode::InvalidArgument => "invalidArgument",
            ErrorCode::Engine => "engine",
            ErrorCode::Limit => "limit",
            ErrorCode::NotSupported => "notSupported",
            ErrorCode::Runtime => "runtime",
            ErrorCode::Protocol => "protocol",
        }
    }
}

/// A refused call, as the runtime sees it. `line` is the script line the shim was running when
/// it made the call, when the runtime can tell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BridgeError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

impl BridgeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        BridgeError { code, message: message.into(), line: None }
    }

    /// A member of the documented object model that GridCraft cannot back. The message names
    /// the class and member so the script's line shows which call to remove.
    pub fn not_supported(member: impl Into<String>) -> Self {
        let member = member.into();
        BridgeError::new(ErrorCode::NotSupported, format!("{member} is not supported by GridCraft's Office Scripts bridge"))
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::BadRequest, message)
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::InvalidArgument, message)
    }

    pub fn engine(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::Engine, message)
    }

    pub fn limit(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::Limit, message)
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::Runtime, message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        BridgeError::new(ErrorCode::Protocol, message)
    }

    pub fn at(mut self, line: Option<u32>) -> Self {
        self.line = line;
        self
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for BridgeError {}

/// How a script failed, as the pane shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    Syntax,
    Runtime,
    Timeout,
    Limit,
    MissingRuntime,
    Protocol,
    Engine,
}

/// A failed script run: the message, the line when known, and what kind of failure it was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptError {
    pub kind: FailureKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

impl ScriptError {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        ScriptError { kind, message: message.into(), line: None }
    }

    pub fn syntax(message: impl Into<String>, line: u32) -> Self {
        ScriptError { kind: FailureKind::Syntax, message: message.into(), line: Some(line) }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        ScriptError::new(FailureKind::Runtime, message)
    }

    pub fn timeout(ms: u64) -> Self {
        ScriptError::new(FailureKind::Timeout, format!("the script did not finish within {ms} ms and was stopped"))
    }

    pub fn limit(message: impl Into<String>) -> Self {
        ScriptError::new(FailureKind::Limit, message)
    }

    pub fn missing_runtime(message: impl Into<String>) -> Self {
        ScriptError::new(FailureKind::MissingRuntime, message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        ScriptError::new(FailureKind::Protocol, message)
    }

    pub fn at(mut self, line: Option<u32>) -> Self {
        self.line = line;
        self
    }

    /// The failing source line, when the message names one in the runtime's diagnostics
    /// (`error: line 3` or `script.ts:3:7`).
    pub fn with_line_from_message(mut self) -> Self {
        if self.line.is_none() {
            self.line = first_line_number(&self.message);
        }
        self
    }
}

impl From<BridgeError> for ScriptError {
    fn from(e: BridgeError) -> Self {
        let kind = match e.code {
            ErrorCode::Limit => FailureKind::Limit,
            ErrorCode::Protocol | ErrorCode::BadRequest => FailureKind::Protocol,
            ErrorCode::NotSupported => FailureKind::Engine,
            _ => FailureKind::Engine,
        };
        ScriptError { kind, message: e.message, line: e.line }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<crate::SyntaxError> for ScriptError {
    fn from(e: crate::SyntaxError) -> Self {
        ScriptError::syntax(e.message, e.line)
    }
}

impl From<crate::StoreError> for ScriptError {
    fn from(e: crate::StoreError) -> Self {
        match e {
            crate::StoreError::Syntax(e) => ScriptError::from(e),
            other => ScriptError::runtime(other.to_string()),
        }
    }
}

/// Parses one request line. Empty lines, oversized lines, invalid JSON and methods that are
/// empty or absurdly long are refused without panicking.
pub fn parse_request(line: &str) -> Result<Request, BridgeError> {
    if line.trim().is_empty() {
        return Err(BridgeError::bad_request("an empty line is not a request"));
    }
    parse_bounded(line)?;
    let request: Request = serde_json::from_str(line).map_err(|e| BridgeError::bad_request(format!("not a bridge request: {e}")))?;
    if request.method.trim().is_empty() || request.method.len() > MAX_METHOD_LEN {
        return Err(BridgeError::bad_request("the method name is empty or too long"));
    }
    Ok(request)
}

/// Parses one reply line, for the runtime's side and for tests.
pub fn parse_response(line: &str) -> Result<Response, BridgeError> {
    if line.trim().is_empty() {
        return Err(BridgeError::protocol("an empty line is not a reply"));
    }
    parse_bounded(line)?;
    // A `null` result is a result: serde reads it as `None`, so the rule is about the raw JSON.
    // `{"result": null, "error": null}` carries neither: a null error is the absence of an error.
    let value: Json = serde_json::from_str(line).map_err(|e| BridgeError::protocol(format!("not a bridge reply: {e}")))?;
    // A present `"result"` key counts even when its value is null (`Some(Null)` and `None`
    // both serialize the same way); a null `"error"` is the absence of an error.
    let has_result = value.get("result").is_some();
    let has_error = value.get("error").is_some_and(|v| !v.is_null());
    if !has_result && !has_error {
        return Err(BridgeError::protocol("a reply carries a result or an error"));
    }
    let mut response: Response = serde_json::from_value(value).map_err(|e| BridgeError::protocol(format!("not a bridge reply: {e}")))?;
    // Serde reads `"result": null` as `None`, which `to_line` would then omit: restore it so
    // the parsed reply serializes back to the line it came from.
    if has_result && response.result.is_none() {
        response.result = Some(Json::Null);
    }
    Ok(response)
}

#[cfg(test)]
mod null_reply_tests {
    use super::*;

    #[test]
    fn a_null_result_round_trips_through_parse_response() {
        // `{"result": null}` is a result, not the absence of one: the parsed reply must carry
        // it as `Some(Null)` so `to_line` writes the same line back.
        let line = Response::ok(1, serde_json::Value::Null).to_line();
        let parsed = parse_response(&line).unwrap();
        assert!(parsed.error.is_none());
        assert_eq!(parsed.result, Some(Json::Null));
        assert_eq!(parsed.to_line(), line);
    }

    #[test]
    fn a_null_error_alone_is_not_a_reply() {
        // A line with neither a result nor an error is refused, even when the `error` key is
        // present with a null value.
        assert!(parse_response(r#"{"id":1,"error":null}"#).is_err());
    }
}

fn parse_bounded(line: &str) -> Result<(), BridgeError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(BridgeError::limit(format!("a bridge line can be at most {MAX_LINE_BYTES} bytes")));
    }
    Ok(())
}

/// The first `line N` or `:N:M` in a message, for a failure reported without a structured line.
fn first_line_number(message: &str) -> Option<u32> {
    let bytes = message.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_digit()) {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let number = message.get(start..i)?.parse::<u32>().ok();
            let after = message.as_bytes().get(i).copied();
            let before = message.get(..start)?;
            let labelled = before.ends_with("line ") || before.ends_with("Line ") || before.ends_with("Line:");
            let position = after == Some(b':') && before.ends_with(':');
            if let Some(n) = number
                && (labelled || position)
            {
                return Some(n);
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_round_trip() {
        let request = Request::new(7, "range.setValue", json!({"range": "A1", "value": 42}));
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(parse_request(&line).unwrap(), request);
        assert_eq!(parse_request(&line).unwrap().call().method, "range.setValue");
    }

    #[test]
    fn responses_round_trip() {
        let ok = Response::ok(1, json!({"sheet": "Sheet1"}));
        assert_eq!(parse_response(&ok.to_line()).unwrap(), ok);
        let failed = Response::failed(2, BridgeError::engine("That cell is locked."));
        assert_eq!(parse_response(&failed.to_line()).unwrap(), failed);
    }

    #[test]
    fn a_reply_needs_a_result_or_an_error() {
        assert!(parse_response("{\"id\":1}").is_err());
    }

    #[test]
    fn malformed_lines_are_errors_not_panics() {
        assert!(parse_request("").is_err());
        assert!(parse_request("   ").is_err());
        assert!(parse_request("{").is_err());
        assert!(parse_request("[]").is_err());
        assert!(parse_request("{\"id\":1,\"method\":\"\"}").is_err());
        let long = format!("{{\"id\":1,\"method\":\"{}\"}}", "a".repeat(MAX_METHOD_LEN + 1));
        assert!(parse_request(&long).is_err());
    }

    #[test]
    fn oversized_lines_are_refused_before_parsing() {
        let mut line = String::from("{\"id\":1,\"method\":\"x\",\"params\":\"");
        line.push_str(&"a".repeat(MAX_LINE_BYTES));
        line.push_str("\"}");
        assert!(matches!(parse_request(&line), Err(BridgeError { code: ErrorCode::Limit, .. })));
    }

    #[test]
    fn engine_errors_map_to_engine_failures_with_their_line() {
        let error = ScriptError::from(BridgeError::engine("no such sheet").at(Some(4)));
        assert_eq!(error.kind, FailureKind::Engine);
        assert_eq!(error.line, Some(4));
        assert_eq!(error.to_string(), "line 4: no such sheet");
    }

    #[test]
    fn lines_are_found_in_runtime_diagnostics() {
        assert_eq!(ScriptError::runtime("error at script.ts:3:9: x is not defined").with_line_from_message().line, Some(3));
        assert_eq!(ScriptError::runtime("line 12: boom").with_line_from_message().line, Some(12));
        assert_eq!(ScriptError::runtime("something else").with_line_from_message().line, None);
    }

    #[test]
    fn a_call_names_its_sheet_and_range() {
        let call = BridgeCall::new("range.setValues", json!({"sheet": "Data", "range": "A1:B2", "values": [[1, 2]]}));
        assert_eq!(call.sheet(), Some("Data"));
        assert_eq!(call.range(), Some("A1:B2"));
    }
}
