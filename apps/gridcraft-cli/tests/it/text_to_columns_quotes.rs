use std::process::Command;

#[test]
fn text_to_columns_preserves_doubled_quotes() {
    let dir = std::env::temp_dir().join(format!("gridcraft-t2c-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("make temp directory");
    let script = dir.join("steps.jsonl");
    std::fs::write(
        &script,
        concat!(
            "{\"command\":\"cell.set\",\"params\":{\"cell\":\"A1\",\"input\":\"\\\"a\\\"\\\"b\\\",c\"}}\n",
            "{\"command\":\"data.textToColumns\",\"params\":{\"range\":\"A1\",\"delimiters\":[\",\"]}}\n",
        ),
    )
    .expect("write commands");
    let output = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli"))
        .args(["run", "--script"])
        .arg(&script)
        .args(["--print", "A1:B1", "--csv", "--quiet"])
        .output()
        .expect("run Gridcraft CLI");
    std::fs::remove_file(script).expect("remove script");
    std::fs::remove_dir(dir).expect("remove temp directory");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "\"a\"\"b\",c");
}
