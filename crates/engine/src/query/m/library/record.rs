//! `Record.*` and `Error.Record`.

use super::super::eval::Ev;
use super::super::value::{MRecord, MResult, MValue};
use super::Args;

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Record.Field" => {
            let r = a.record(0)?;
            let n = a.text(1)?;
            ev.field(&MValue::Record(r), &n, false)?
        }
        "Record.FieldNames" => MValue::list(a.record(0)?.names.iter().map(|n| MValue::Text(n.clone())).collect()),
        "Record.FieldValues" => MValue::list(a.record(0)?.values.clone()),
        "Record.HasFields" => {
            let r = a.record(0)?;
            MValue::Logical(super::names_of(a.at(1))?.iter().all(|n| r.get(n).is_some()))
        }
        "Error.Record" => MValue::record(MRecord::new(vec![
            ("Reason".into(), a.at(0).clone()),
            ("Message".into(), a.at(1).clone()),
            ("Detail".into(), a.at(2).clone()),
        ])),
        _ => return Ok(None),
    };
    Ok(Some(r))
}
