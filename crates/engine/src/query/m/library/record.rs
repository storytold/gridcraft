//! `Record.*` and `Error.Record`.

use super::super::eval::Ev;
use super::super::value::{MError, MRecord, MResult, MTable, MValue};
use super::Args;
use crate::query::MAX_ROWS;

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Record.Field" => {
            let r = a.record(0)?;
            let n = a.text(1)?;
            ev.field(&MValue::Record(r), &n, false)?
        }
        "Record.FieldNames" => MValue::list(a.record(0)?.names.iter().map(|n| MValue::Text(n.clone())).collect()),
        "Record.FieldValues" => MValue::list(a.record(0)?.values.clone()),
        "Record.ToTable" => {
            let r = a.record(0)?;
            // A field becomes a row. Refuse oversized input before allocating output rows.
            if r.names.len() > MAX_ROWS {
                return Err(MError::expr(format!("A table can hold at most {MAX_ROWS} rows here.")));
            }
            if r.names.len() != r.values.len() {
                return Err(MError::expr("The record has inconsistent field names and values."));
            }
            ev.tick(r.names.len() as u64)?;
            let mut table = MTable::new(vec!["Name".into(), "Value".into()]);
            for (name, value) in r.names.iter().zip(&r.values) {
                // Copy the value, including held errors and deferred sources, without forcing it.
                table.push(vec![MValue::Text(name.clone()), value.clone()])?;
            }
            MValue::table(table)
        }
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
