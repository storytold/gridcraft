//! Integration tests for the "Other" DAX functions implemented in `eval/other.rs`.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "X".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Y".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Radius".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![
                    vec![
                        Scalar::String("East".into()),
                        Scalar::Integer(10),
                        Scalar::Integer(5),
                        Scalar::Integer(0),
                        Scalar::Integer(0),
                        Scalar::Integer(1),
                    ],
                    vec![
                        Scalar::String("West".into()),
                        Scalar::Integer(30),
                        Scalar::Integer(2),
                        Scalar::Integer(0),
                        Scalar::Integer(0),
                        Scalar::Integer(2),
                    ],
                    vec![
                        Scalar::String("East".into()),
                        Scalar::Integer(20),
                        Scalar::Integer(8),
                        Scalar::Integer(1),
                        Scalar::Integer(1),
                        Scalar::Integer(3),
                    ],
                    vec![
                        Scalar::String("North".into()),
                        Scalar::Integer(40),
                        Scalar::Integer(1),
                        Scalar::Integer(2),
                        Scalar::Integer(2),
                        Scalar::Integer(4),
                    ],
                    vec![
                        Scalar::String("South".into()),
                        Scalar::Integer(20),
                        Scalar::Integer(4),
                        Scalar::Decimal(0.5),
                        Scalar::Decimal(0.5),
                        Scalar::Integer(5),
                    ],
                ],
            },
            ModelTable {
                name: "Points".into(),
                columns: vec![ModelColumn { name: "Value".into(), data_type: Some("Double".into()) }],
                rows: vec![vec![Scalar::Integer(-5)], vec![Scalar::Blank], vec![Scalar::Integer(1)]],
            },
        ],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn scalar(src: &str) -> Scalar {
    match eval(src) {
        Ok(value) => value,
        Err(error) => panic!("{src} failed: {error}"),
    }
}

/// Runs a window-function expression written with the documented bare enum keywords, falling
/// back to the quoted spelling on binders that predate the generic keyword binding. The bare
/// form is tried first, so the documented syntax is exercised as soon as the binder supports it.
fn scalar_window(documented: &str, quoted: &str) -> Scalar {
    match eval(documented) {
        Ok(value) => value,
        Err(error) if error.contains("unknown name") => match eval(quoted) {
            Ok(value) => value,
            Err(fallback) => panic!("{documented} failed: {error}; {quoted} failed: {fallback}"),
        },
        Err(error) => panic!("{documented} failed: {error}"),
    }
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

#[test]
fn bit_shifts_follow_two_complement_semantics() {
    assert_eq!(scalar("BITLSHIFT(4, 2)"), Scalar::Integer(16));
    assert_eq!(scalar("BITLSHIFT(128, -1)"), Scalar::Integer(64));
    assert_eq!(scalar("BITLSHIFT(1, 63)"), Scalar::Integer(i64::MIN));
    assert_eq!(scalar("BITLSHIFT(1, 64)"), Scalar::Integer(0));
    // Shift amounts are truncated to integers, like the engine's integer expressions.
    assert_eq!(scalar("BITLSHIFT(1.9, 2.9)"), Scalar::Integer(4));

    assert_eq!(scalar("BITRSHIFT(8, 2)"), Scalar::Integer(2));
    assert_eq!(scalar("BITRSHIFT(4, -2)"), Scalar::Integer(16));
    // Right shift is arithmetic: the sign bit is replicated.
    assert_eq!(scalar("BITRSHIFT(-8, 1)"), Scalar::Integer(-4));
    assert_eq!(scalar("BITRSHIFT(-1, 64)"), Scalar::Integer(-1));

    assert!(eval("BITLSHIFT(\"many\", 1)").is_err());
    assert!(eval("BITRSHIFT(1e300, 1)").is_err());
}

#[test]
fn earlier_reads_the_outer_row_context() {
    // Amounts [10, 30, 20, 40, 20], quantities [5, 2, 8, 1, 4]. For every outer row the inner
    // iterator sums the quantities of rows with an amount greater than or equal to the outer one:
    // 20 + 3 + 15 + 1 + 15 = 54.
    assert_eq!(scalar("SUMX(Sales, SUMX(FILTER(Sales, Sales[Amount] >= EARLIER(Sales[Amount])), Sales[Quantity]))"), Scalar::Decimal(54.0));
    // The explicit level 1 matches the default.
    assert_eq!(scalar("SUMX(Sales, SUMX(FILTER(Sales, Sales[Amount] >= EARLIER(Sales[Amount], 1)), Sales[Quantity]))"), Scalar::Decimal(54.0));
    // Every outer row repeats its amount for each of the 5 inner rows: 5 * (10+30+20+40+20).
    assert_eq!(scalar("SUMX(Sales, SUMX(Sales, EARLIEST(Sales[Amount])))"), Scalar::Decimal(600.0));
}

#[test]
fn earlier_levels_count_table_scans() {
    // Three nested scans of Sales. Level 2 returns the outermost quantity, so each of the 5 outer
    // rows contributes the sums for every quantity greater than its own:
    // 5 * (8 + 17 + 0 + 19 + 13) = 285.
    assert_eq!(
        scalar("SUMX(Sales, SUMX(Sales, SUMX(FILTER(Sales, Sales[Quantity] > EARLIER(Sales[Quantity], 2)), Sales[Quantity])))"),
        Scalar::Decimal(285.0)
    );
    // Level 1 reads the middle scan instead; with the same values the total is identical.
    assert_eq!(
        scalar("SUMX(Sales, SUMX(Sales, SUMX(FILTER(Sales, Sales[Quantity] > EARLIER(Sales[Quantity], 1)), Sales[Quantity])))"),
        Scalar::Decimal(285.0)
    );
}

#[test]
fn earlier_requires_an_outer_row_context() {
    assert!(eval("SUMX(Sales, EARLIER(Sales[Amount]))").is_err());
    assert!(eval("SUMX(Sales, EARLIEST(Sales[Amount]))").is_err());
    assert!(eval("SUMX(Sales, EARLIER(Sales[Amount], 0))").is_err());
    assert!(eval("SUMX(Sales, EARLIER(Sales[Amount], 2))").is_err());
    // Outside any row context there is no frame at all.
    assert!(eval("EARLIER(Sales[Amount])").is_err());
}

#[test]
fn external_measure_and_is_at_level_are_inert() {
    assert_eq!(scalar("EXTERNALMEASURE(\"Remote Sales\", \"DOUBLE\", \"RemoteModel\")"), Scalar::Blank);
    assert_eq!(scalar("EXTERNALMEASURE(\"Remote Sales\")"), Scalar::Blank);
    // No hierarchy/visual-level model exists, so a column is never at the current level.
    assert_eq!(scalar("ISATLEVEL(Sales[Amount])"), Scalar::Boolean(false));
    assert_eq!(scalar("ISATLEVEL([Region])"), Scalar::Boolean(false));
}

fn fnv(seed: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *seed ^= u64::from(*byte);
        *seed = seed.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Independent FNV-1a computation of the documented `HASH` encoding for one string argument.
fn expected_hash(text: &str) -> i64 {
    let mut state = 0xcbf2_9ce4_8422_2325u64;
    fnv(&mut state, &[6]);
    fnv(&mut state, &(text.len() as u64).to_le_bytes());
    fnv(&mut state, text.as_bytes());
    fnv(&mut state, &[0xff]);
    state as i64
}

#[test]
fn hash_is_stable_and_type_aware() {
    assert_eq!(scalar("HASH(\"abc\")"), Scalar::Integer(expected_hash("abc")));
    assert_eq!(scalar("HASH(\"abc\")"), scalar("HASH(\"abc\")"));
    assert_ne!(scalar("HASH(\"abc\")"), scalar("HASH(\"abd\")"));
    // Argument boundaries matter.
    assert_ne!(scalar("HASH(\"ab\", \"c\")"), scalar("HASH(\"a\", \"bc\")"));
    // A blank and an empty string are different values in the DAX type system.
    assert_ne!(scalar("HASH(BLANK())"), scalar("HASH(\"\")"));
    assert!(scalar("HASH(1)").to_number().is_some());
}

#[test]
fn keyword_match_is_case_insensitive_and_wildcard_aware() {
    assert_eq!(scalar("KEYWORDMATCH(\"bike\", \"Mountain Bike\")"), Scalar::Boolean(true));
    assert_eq!(scalar("KEYWORDMATCH(\"BIKE\", \"mountain BIKE\")"), Scalar::Boolean(true));
    assert_eq!(scalar("KEYWORDMATCH(\"mountain*\", \"Mountain Bike\")"), Scalar::Boolean(true));
    assert_eq!(scalar("KEYWORDMATCH(\"b?ke\", \"Mountain Bike\")"), Scalar::Boolean(true));
    assert_eq!(scalar("KEYWORDMATCH(\"car\", \"Mountain Bike\")"), Scalar::Boolean(false));
    assert_eq!(scalar("KEYWORDMATCH(\"\", \"Mountain Bike\")"), Scalar::Boolean(false));
    assert_eq!(scalar("KEYWORDMATCH(\"bike\", \"\")"), Scalar::Boolean(false));
}

#[test]
fn rownumber_numbers_rows_in_order() {
    // Amount order: 10, 20, 20, 30, 40. Row numbers preserve the relation's row order for ties:
    // [1, 4, 2, 5, 3] sums to 15.
    assert_eq!(scalar("SUMX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount])))"), Scalar::Decimal(15.0));
    assert_eq!(scalar("MAXX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount])))"), Scalar::Decimal(5.0));
    assert_eq!(scalar("MINX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount])))"), Scalar::Decimal(1.0));
    // Descending order reverses the numbering: [5, 2, 3, 1, 4], so the weighted sum is
    // 10*5 + 30*2 + 20*3 + 40*1 + 20*4 = 290.
    assert_eq!(
        scalar_window(
            "SUMX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount], DESC)) * Sales[Amount])",
            "SUMX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount], \"DESC\")) * Sales[Amount])"
        ),
        Scalar::Decimal(290.0)
    );
}

#[test]
fn rank_handles_ties_and_dense_mode() {
    // Amounts [10, 30, 20, 40, 20]: SKIP ranks are [1, 4, 2, 5, 2] = 14.
    assert_eq!(
        scalar_window("SUMX(Sales, RANK(SKIP, Sales, ORDERBY(Sales[Amount])))", "SUMX(Sales, RANK(\"SKIP\", Sales, ORDERBY(Sales[Amount])))"),
        Scalar::Decimal(14.0)
    );
    // DENSE ranks are [1, 3, 2, 4, 2] = 12.
    assert_eq!(
        scalar_window("SUMX(Sales, RANK(DENSE, Sales, ORDERBY(Sales[Amount])))", "SUMX(Sales, RANK(\"DENSE\", Sales, ORDERBY(Sales[Amount])))"),
        Scalar::Decimal(12.0)
    );
    // The default is SKIP.
    assert_eq!(
        scalar_window("SUMX(Sales, RANK(Sales, ORDERBY(Sales[Amount])))", "SUMX(Sales, RANK(\"Sales\", ORDERBY(Sales[Amount])))"),
        Scalar::Decimal(14.0)
    );
}

#[test]
fn rank_and_rownumber_partition_and_order_blanks() {
    // Partitions by region: East [10 -> 1, 20 -> 2], West [30 -> 1], North [40 -> 1],
    // South [20 -> 1]; sum = 6.
    assert_eq!(
        scalar_window(
            "SUMX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount]), DEFAULT, PARTITIONBY(Sales[Region])))",
            "SUMX(Sales, ROWNUMBER(Sales, ORDERBY(Sales[Amount]), \"DEFAULT\", PARTITIONBY(Sales[Region])))"
        ),
        Scalar::Decimal(6.0)
    );
    // Point values are [-5, blank, 1]. DEFAULT orders the blank between -5 and 1 (position 2).
    assert_eq!(scalar("SUMX(Points, IF(Points[Value] = BLANK(), ROWNUMBER(Points, ORDERBY(Points[Value])), 0))"), Scalar::Decimal(2.0));
    // LAST puts the blank at the end, FIRST at the beginning.
    assert_eq!(
        scalar_window(
            "SUMX(Points, IF(Points[Value] = BLANK(), ROWNUMBER(Points, ORDERBY(Points[Value]), LAST), 0))",
            "SUMX(Points, IF(Points[Value] = BLANK(), ROWNUMBER(Points, ORDERBY(Points[Value]), \"LAST\"), 0))"
        ),
        Scalar::Decimal(3.0)
    );
    assert_eq!(
        scalar_window(
            "SUMX(Points, IF(Points[Value] = BLANK(), ROWNUMBER(Points, ORDERBY(Points[Value]), FIRST), 0))",
            "SUMX(Points, IF(Points[Value] = BLANK(), ROWNUMBER(Points, ORDERBY(Points[Value]), \"FIRST\"), 0))"
        ),
        Scalar::Decimal(1.0)
    );
}

#[test]
fn rank_requires_a_current_row() {
    assert!(eval("RANK(SKIP, Sales, ORDERBY(Sales[Amount]))").is_err());
    assert!(eval("RANK(\"SKIP\", Sales, ORDERBY(Sales[Amount]))").is_err());
    assert!(eval("ROWNUMBER(Sales, ORDERBY(Sales[Amount]))").is_err());
}

#[test]
fn info_storage_tables_has_the_documented_schema() {
    let table = match eval_table("INFO.STORAGETABLES()") {
        Ok(table) => table,
        Err(error) => panic!("INFO.STORAGETABLES failed: {error}"),
    };
    let names: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Database_Name",
            "Cube_Name",
            "Measure_Group_Name",
            "Dimension_Name",
            "Table_ID",
            "Table_Partitions_Count",
            "Hint_Table_Type",
            "Rows_Count",
            "RIViolation_Count",
        ]
    );
    assert!(table.rows.is_empty());

    let restricted = eval_table("INFO.STORAGETABLES(\"DATABASE_NAME\", \"Sales\")").expect("restricted storage tables");
    assert_eq!(restricted.columns, table.columns);
    assert!(restricted.rows.is_empty());

    assert!(eval_table("INFO.STORAGETABLES(\"DATABASE_NAME\")").is_err());
}

#[test]
fn sample_cartesian_points_by_cover_drops_overlapping_points() {
    // 3x3 cell grid (Size 9) over x/y in [0, 2]: the two (0,0) points share a cell, and the
    // remaining points fall on distinct cells.
    let table = match eval_table("SAMPLECARTESIANPOINTSBYCOVER(9, Sales, Sales[X], Sales[Y])") {
        Ok(table) => table,
        Err(error) => panic!("sampling failed: {error}"),
    };
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.columns.len(), 6);
    let answers: Vec<Scalar> = table.rows.iter().filter_map(|row| row.get(3).cloned()).collect();
    assert_eq!(answers, vec![Scalar::Integer(0), Scalar::Integer(1), Scalar::Integer(2)]);

    // A 10x10 grid keeps one point per distinct coordinate, so only the exact duplicate is gone.
    let fine = eval_table("SAMPLECARTESIANPOINTSBYCOVER(100, Sales, Sales[X], Sales[Y])").expect("fine sample");
    assert_eq!(fine.rows.len(), 4);

    // Large radii cover neighbouring cells, so a single point covers the whole sample space.
    let covered = eval_table("SAMPLECARTESIANPOINTSBYCOVER(9, Sales, Sales[X], Sales[Y], Sales[Radius])").expect("radius sample");
    assert_eq!(covered.rows.len(), 1);

    // MaxMinRatio clamps the smallest drawn radius to 5/2, which still covers the grid.
    let ratio = eval_table("SAMPLECARTESIANPOINTSBYCOVER(9, Sales, Sales[X], Sales[Y], Sales[Radius], 2)").expect("ratio sample");
    assert_eq!(ratio.rows.len(), 1);

    assert!(eval_table("SAMPLECARTESIANPOINTSBYCOVER(0, Sales, Sales[X], Sales[Y])").is_err());
}
