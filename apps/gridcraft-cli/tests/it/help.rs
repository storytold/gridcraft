use std::process::Command;

#[test]
fn help_flags_exit_successfully_for_top_level_and_subcommands() {
    for args in [vec!["--help"], vec!["-h"], vec!["info", "--help"], vec!["run", "-h"], vec!["mcp", "--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).args(&args).output().expect("launch CLI help");
        assert!(output.status.success(), "{args:?}: {:?}", output.status);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("gridcraft-cli info"), "{args:?}: {stdout}");
        assert!(output.stderr.is_empty(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn an_unknown_option_prints_usage_to_stderr_and_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).args(["eval", "1", "--unknown-option"]).output().expect("launch CLI");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option --unknown-option"), "{stderr}");
    assert!(stderr.contains("gridcraft-cli info"), "{stderr}");
}
