//! Writes a workbook exercising every x14 conditional-format extension — solid bar with
//! border/negative colours/axis/length limits, a bar with hidden values (`showValue="0"`
//! mirrored on both sides), a bar with non-default length limits, custom icon set, `5Boxes`
//! (x14-only), plus a sparkline — for a manual open-in-Excel check: it must open with no
//! repair prompt.
//!
//! Usage: `cargo run -p gridcraft-xlsx --example x14-sample -- [out.xlsx]`
//! (default `/tmp/opencode/x14-writer-sample.xlsx`).

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{CfRule, CfValueKind, Color, CondFormat, IconChoice, Sparkline, SparklineKind, Workbook, default_icon_thresholds};
use gridcraft_xlsx::{read_xlsx, write_xlsx};

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) if !p.is_empty() => p,
        _ => "/tmp/opencode/x14-writer-sample.xlsx".into(),
    };
    if let Err(e) = run(&path) {
        eprintln!("x14-sample: {e}");
        std::process::exit(1);
    }
}

fn run(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).ok_or("no sheet")?;
    for (i, v) in [-40.0, -12.0, 0.0, 5.0, 9.0, 33.0, 50.0, 67.0, 100.0].into_iter().enumerate() {
        s.set_value(CellRef::new(i as u32 + 1, 1), Value::Number(v));
    }
    // Solid bar with every extension field: border, negative fill and border, axis, limits.
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("B2:B10").ok_or("range")?],
        rule: CfRule::DataBar {
            min: CfValueKind::Number(0.0),
            max: CfValueKind::Number(100.0),
            color: Color::Rgb(0x638EC6),
            gradient: false,
            show_value: true,
            border: Some(Color::Rgb(0xFF0000)),
            negative_fill: Some(Color::Rgb(0xFF00FF)),
            negative_border: Some(Color::Rgb(0x0000FF)),
            axis: Some(Color::Rgb(0x00FF00)),
            limits: Some((10, 90)),
        },
        priority: 1,
        stop_if_true: false,
    });
    // Plain gradient bar (must stay classic-only, no extension).
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("C2:C10").ok_or("range")?],
        rule: CfRule::DataBar {
            min: CfValueKind::Min,
            max: CfValueKind::Max,
            color: Color::Rgb(0x638EC6),
            gradient: true,
            show_value: true,
            border: None,
            negative_fill: None,
            negative_border: None,
            axis: None,
            limits: None,
        },
        priority: 2,
        stop_if_true: false,
    });
    // Bare solid bar: solid fill is x14-only, everything else default (no axis, no flags).
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("G2:G10").ok_or("range")?],
        rule: CfRule::DataBar {
            min: CfValueKind::Min,
            max: CfValueKind::Max,
            color: Color::Rgb(0x638EC6),
            gradient: false,
            show_value: true,
            border: None,
            negative_fill: None,
            negative_border: None,
            axis: None,
            limits: None,
        },
        priority: 5,
        stop_if_true: false,
    });
    // Solid bar with hidden values: `showValue="0"` must appear on both the classic bar and
    // the x14 refinement, or Excel merging the x14 side would bring the values back.
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("H2:H10").ok_or("range")?],
        rule: CfRule::DataBar {
            min: CfValueKind::Min,
            max: CfValueKind::Max,
            color: Color::Rgb(0x638EC6),
            gradient: false,
            show_value: false,
            border: None,
            negative_fill: None,
            negative_border: None,
            axis: None,
            limits: None,
        },
        priority: 6,
        stop_if_true: false,
    });
    // Solid bar with non-default length limits (bars occupy the middle 5–95% of the cell).
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("I2:I10").ok_or("range")?],
        rule: CfRule::DataBar {
            min: CfValueKind::Min,
            max: CfValueKind::Max,
            color: Color::Rgb(0x638EC6),
            gradient: false,
            show_value: true,
            border: None,
            negative_fill: None,
            negative_border: None,
            axis: None,
            limits: Some((5, 95)),
        },
        priority: 7,
        stop_if_true: false,
    });
    // Custom icon set: 3Arrows with every position drawn from 3Triangles.
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("D2:D10").ok_or("range")?],
        rule: CfRule::IconSet {
            set: "3Arrows".into(),
            thresholds: default_icon_thresholds("3Arrows"),
            reverse: false,
            show_value: true,
            icons: Some((0..3).map(|id| IconChoice { set: "3Triangles".into(), id }).collect()),
        },
        priority: 3,
        stop_if_true: false,
    });
    // 5Boxes exists only in x14: an x14-only rule.
    s.cond_formats.push(CondFormat {
        ranges: vec![gridcraft_core::RangeRef::parse("E2:E10").ok_or("range")?],
        rule: CfRule::IconSet { set: "5Boxes".into(), thresholds: default_icon_thresholds("5Boxes"), reverse: false, show_value: true, icons: None },
        priority: 4,
        stop_if_true: false,
    });
    s.sparklines.push(Sparkline {
        cell: CellRef::parse("F2").ok_or("cell")?,
        source: "Sheet1!B2:B10".into(),
        kind: SparklineKind::Line,
        color: Color::Rgb(0x376092),
        markers: true,
    });

    let bytes = write_xlsx(&wb)?;
    std::fs::write(path, &bytes)?;
    // Read it back: no warnings, everything intact.
    let (back, rep) = read_xlsx(&bytes)?;
    let sheet = back.sheet(0).ok_or("no sheet read back")?;
    if !rep.warnings.is_empty() {
        return Err(format!("warnings on read-back: {:?}", rep.warnings).into());
    }
    let want = wb.sheet(0).ok_or("no sheet")?.cond_formats.clone();
    if sheet.cond_formats != want {
        return Err("conditional formats did not survive the round trip".into());
    }
    println!("wrote {path} ({} bytes, {} conditional formats, {} sparkline)", bytes.len(), sheet.cond_formats.len(), sheet.sparklines.len());
    Ok(())
}
