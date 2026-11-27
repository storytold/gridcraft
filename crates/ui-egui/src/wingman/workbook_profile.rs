//! Profile cached, typed values without evaluating any cell or exporting its contents.
use super::context;
use gridcraft_engine::{
    DocState,
    core::{RangeRef, Value, col_to_letters},
};
use serde_json::{Value as Json, json};

#[derive(Default)]
struct Column {
    counts: [u32; 6],
    examples: [Option<String>; 6],
    numeric_text: u32,
    sample_numeric_text: Option<String>,
}

pub(super) fn inspect(d: &DocState, args: &Json) -> Result<Json, String> {
    let sh = context::sheet(d, args)?;
    let r = args["range"].as_str().and_then(RangeRef::parse).ok_or("Supply a rectangular range.")?;
    if r.width() > 64 {
        return Err("Profile at most 64 columns at a time.".into());
    }
    let offset = args["offset"].as_u64().unwrap_or(0);
    if offset >= u64::from(r.height()) {
        return Err("Row offset is outside the requested range.".into());
    }
    let start = r.start.row + offset as u32;
    let rows = (8192 / r.width()).min(r.end.row - start + 1);
    let mut columns: Vec<_> = (0..r.width()).map(|_| Column::default()).collect();
    for row in start..start + rows {
        for (i, column) in columns.iter_mut().enumerate() {
            let at = gridcraft_engine::core::CellRef::new(row, r.start.col + i as u32);
            let kind = match sh.value_ref(at) {
                None | Some(Value::Empty) => 0,
                Some(Value::Number(_)) => 1,
                Some(Value::Text(t)) => {
                    if t.len() <= 128 && t.trim().parse::<f64>().is_ok_and(f64::is_finite) {
                        column.numeric_text += 1;
                        column.sample_numeric_text.get_or_insert_with(|| at.a1());
                    }
                    2
                }
                Some(Value::Bool(_)) => 3,
                Some(Value::Error(_)) => 4,
                Some(Value::Array(_)) => 5,
            };
            column.counts[kind] += 1;
            column.examples[kind].get_or_insert_with(|| at.a1());
        }
    }
    let columns=columns.into_iter().enumerate().map(|(i,c)|{
        let mut types=json!({});
        for (k,name) in ["blank","number","text","boolean","error","array"].iter().enumerate(){
            if c.counts[k]>0{types[*name]=json!({"count":c.counts[k],"example":c.examples[k]});}
        }
        json!({"column":col_to_letters(r.start.col+i as u32),"types":types,"numericLookingText":c.numeric_text,"exampleNumericText":c.sample_numeric_text})
    }).collect::<Vec<_>>();
    let next = offset + u64::from(rows);
    Ok(json!({"sheet":sh.name,"range":r.a1(),"firstRow":start+1,"lastRow":start+rows,"columns":columns,
        "nextOffset":(next<u64::from(r.height())).then_some(next),"source":"Cached values; dates and currency are numbers with display formats. Counts are for these rows only."}))
}
